//! 맥 단축키 실행기와 설정 창 도우미. NSWorkspace, NSOpenPanel, NSBundle, 단축어 앱을 부른다.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use objc2::MainThreadMarker;
use objc2::rc::autoreleasepool;
use objc2_app_kit::{NSModalResponseOK, NSOpenPanel, NSWorkspace};
use objc2_foundation::{NSBundle, NSString, NSURL};
use sidedesk_domain::{
    Action, AppChoice, Combo, Launcher, PortError, ShortcutHelp, SystemAction, check_path,
    check_url,
};

use crate::{audio, keys};

const SHORTCUTS: &str = "/usr/bin/shortcuts";
const SHORTCUTS_LIMIT: Duration = Duration::from_secs(10);
const KEEP: usize = 64 * 1024;
const KEY_SETTINGS: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";

pub struct MacLauncher {
    /// 방해 금지는 우리가 마지막으로 바꾼 값만 안다. 켜기, 끄기 단축어 쌍마다 따로.
    focus: Mutex<HashMap<(String, String), bool>>,
    children: Mutex<HashMap<u32, Child>>,
    stopped: AtomicBool,
    home: PathBuf,
    tool: PathBuf,
    limit: Duration,
}

impl MacLauncher {
    pub fn new(home: PathBuf) -> Self {
        Self::with_tool(home, PathBuf::from(SHORTCUTS), SHORTCUTS_LIMIT)
    }

    /// 시험에서 단축어 프로그램과 시간 제한을 바꾼다.
    pub fn with_tool(home: PathBuf, tool: PathBuf, limit: Duration) -> Self {
        Self {
            focus: Mutex::default(),
            children: Mutex::default(),
            stopped: AtomicBool::new(false),
            home,
            tool,
            limit,
        }
    }

    /// 쉘 없이 정해진 인자로 돌린다. 시간이 넘으면 끝내고 거둔다.
    /// `capture` 가 아니면 출력을 받지 않는다(단축어가 띄운 손자가 파이프를 물고 남아도 막히지 않게).
    fn shortcuts(&self, args: &[&str], capture: bool) -> Option<(bool, Vec<u8>)> {
        if self.stopped.load(Ordering::SeqCst) {
            return None;
        }
        let mut child = Command::new(&self.tool)
            .args(args)
            .current_dir(&self.home)
            .stdin(Stdio::null())
            .stdout(if capture {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let pid = child.id();
        let out = child.stdout.take().map(|mut pipe| {
            thread::spawn(move || {
                let mut kept = Vec::new();
                let mut buffer = [0u8; 8192];
                while let Ok(n) = pipe.read(&mut buffer) {
                    if n == 0 {
                        break;
                    }
                    let room = KEEP.saturating_sub(kept.len());
                    kept.extend_from_slice(&buffer[..n.min(room)]);
                }
                kept
            })
        });
        {
            let mut children = self.lock_children();
            children.insert(pid, child);
            // 띄우는 사이 stop 이 왔으면 바로 끝낸다.
            if self.stopped.load(Ordering::SeqCst)
                && let Some(mut child) = children.remove(&pid)
            {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        let start = Instant::now();
        let ok = loop {
            let mut children = self.lock_children();
            let Some(child) = children.get_mut(&pid) else {
                break false;
            };
            match child.try_wait() {
                Ok(Some(status)) => {
                    children.remove(&pid);
                    break status.success();
                }
                Ok(None) if start.elapsed() < self.limit => {}
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    children.remove(&pid);
                    break false;
                }
            }
            drop(children);
            thread::sleep(Duration::from_millis(20));
        };
        let output = out.and_then(|h| h.join().ok()).unwrap_or_default();
        Some((ok, output))
    }

    fn lock_children(&self) -> std::sync::MutexGuard<'_, HashMap<u32, Child>> {
        self.children
            .lock()
            .unwrap_or_else(|lock| lock.into_inner())
    }

    fn lock_focus(&self) -> std::sync::MutexGuard<'_, HashMap<(String, String), bool>> {
        self.focus.lock().unwrap_or_else(|lock| lock.into_inner())
    }

    /// 상태 잠금은 읽고 쓸 때만 쥔다(단축어가 도는 10초 동안 상태 물음을 막지 않게).
    fn focus(&self, on: &str, off: &str) -> Result<(), PortError> {
        let pair = (on.to_owned(), off.to_owned());
        // 모르면 켠다.
        let turn_on = self.lock_focus().get(&pair) != Some(&true);
        let name = if turn_on { on } else { off };
        if name.starts_with('-') {
            return Err(PortError::new("단축어를 찾지 못했습니다"));
        }
        match self.shortcuts(&["run", name], false) {
            Some((true, _)) => {
                self.lock_focus().insert(pair, turn_on);
                Ok(())
            }
            _ => Err(PortError::new("단축어를 찾지 못했습니다")),
        }
    }
}

fn open_url(url: &NSURL) -> bool {
    NSWorkspace::sharedWorkspace().openURL(url)
}

impl Launcher for MacLauncher {
    fn run(&self, action: &Action) -> Result<(), PortError> {
        autoreleasepool(|_| match action {
            Action::App { bundle, .. } => {
                let found = NSWorkspace::sharedWorkspace()
                    .URLForApplicationWithBundleIdentifier(&NSString::from_str(bundle));
                match found {
                    Some(url) if open_url(&url) => Ok(()),
                    Some(_) => Err(PortError::new("실행하지 못했습니다")),
                    None => Err(PortError::new("앱을 찾지 못했습니다")),
                }
            }
            Action::Open { path } => {
                check_path(path).map_err(PortError::new)?;
                if !Path::new(path).exists() {
                    return Err(PortError::new("파일을 찾지 못했습니다"));
                }
                let url = NSURL::fileURLWithPath(&NSString::from_str(path));
                if open_url(&url) {
                    Ok(())
                } else {
                    Err(PortError::new("파일을 찾지 못했습니다"))
                }
            }
            Action::Url { url } => {
                check_url(url).map_err(PortError::new)?;
                match NSURL::URLWithString(&NSString::from_str(url)) {
                    Some(url) if open_url(&url) => Ok(()),
                    _ => Err(PortError::new("주소를 열지 못했습니다")),
                }
            }
            Action::Keys { combo } => keys::send(combo),
            Action::System {
                action: SystemAction::Mute,
            } => audio::toggle_mute(),
            Action::System {
                action: SystemAction::Lock,
            } => keys::send(&Combo {
                code: "KeyQ".into(),
                command: true,
                control: true,
                option: false,
                shift: false,
            }),
            Action::System {
                action: SystemAction::Focus { on, off },
            } => self.focus(on, off),
        })
    }

    fn toggle(&self, action: &Action) -> Option<bool> {
        match action {
            Action::System {
                action: SystemAction::Mute,
            } => audio::muted(),
            Action::System {
                action: SystemAction::Focus { on, off },
            } => self.lock_focus().get(&(on.clone(), off.clone())).copied(),
            _ => None,
        }
    }

    fn can_send_keys(&self) -> bool {
        keys::allowed()
    }

    fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        for (_, mut child) in self.lock_children().drain() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl ShortcutHelp for MacLauncher {
    fn apps(&self) -> Vec<AppChoice> {
        let folders = [
            PathBuf::from("/Applications"),
            PathBuf::from("/Applications/Utilities"),
            PathBuf::from("/System/Applications"),
            PathBuf::from("/System/Applications/Utilities"),
            self.home.join("Applications"),
        ];
        let mut apps: Vec<AppChoice> = folders
            .iter()
            .filter_map(|folder| std::fs::read_dir(folder).ok())
            .flat_map(|entries| entries.flatten())
            .filter(|entry| entry.path().extension().is_some_and(|e| e == "app"))
            .filter_map(|entry| autoreleasepool(|_| app_choice(&entry.path())))
            .collect();
        apps.sort_by_key(|app| app.name.to_lowercase());
        apps.dedup_by(|a, b| a.bundle == b.bundle);
        apps
    }

    fn pick_path(&self) -> Option<String> {
        let mtm = MainThreadMarker::new()?;
        autoreleasepool(|_| {
            let panel = NSOpenPanel::openPanel(mtm);
            panel.setCanChooseFiles(true);
            panel.setCanChooseDirectories(true);
            panel.setAllowsMultipleSelection(false);
            if panel.runModal() != NSModalResponseOK {
                return None;
            }
            let url = panel.URLs().firstObject()?;
            url.path().map(|path| path.to_string())
        })
    }

    fn focus_shortcuts(&self) -> Vec<String> {
        match self.shortcuts(&["list"], true) {
            Some((true, output)) => String::from_utf8_lossy(&output)
                .lines()
                .map(str::trim)
                .filter(|name| !name.is_empty() && !name.starts_with('-'))
                .map(str::to_owned)
                .collect(),
            _ => Vec::new(),
        }
    }

    fn app_exists(&self, bundle: &str) -> bool {
        autoreleasepool(|_| {
            NSWorkspace::sharedWorkspace()
                .URLForApplicationWithBundleIdentifier(&NSString::from_str(bundle))
                .is_some()
        })
    }

    fn open_key_settings(&self) {
        autoreleasepool(|_| {
            if let Some(url) = NSURL::URLWithString(&NSString::from_str(KEY_SETTINGS)) {
                open_url(&url);
            }
        });
    }
}

fn app_choice(path: &Path) -> Option<AppChoice> {
    let bundle = NSBundle::bundleWithPath(&NSString::from_str(path.to_str()?))?;
    let id = bundle.bundleIdentifier()?.to_string();
    let name = path.file_stem()?.to_str()?.to_owned();
    let (icon, color) = guess(&name);
    Some(AppChoice {
        name,
        bundle: id,
        icon: icon.to_owned(),
        color: color.to_owned(),
    })
}

/// 앱 이름으로 그림과 색을 짐작한다.
pub fn guess(name: &str) -> (&'static str, &'static str) {
    let lower = name.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| lower.contains(w));
    if has(&["terminal", "iterm", "warp", "ghostty", "alacritty", "kitty"]) {
        ("terminal", "slate")
    } else if has(&["finder", "files"]) {
        ("folder", "green")
    } else if has(&["code", "xcode", "intellij", "zed", "sublime", "cursor"]) {
        ("code", "purple")
    } else if has(&[
        "safari", "chrome", "firefox", "edge", "arc", "brave", "browser",
    ]) {
        ("browser", "orange")
    } else if has(&["calculator"]) {
        ("calculator", "teal")
    } else if has(&["settings", "preferences"]) {
        ("settings", "gray")
    } else if has(&["activity monitor"]) {
        ("activity", "olive")
    } else if has(&["music", "spotify"]) {
        ("music", "rose")
    } else {
        ("star", "sky")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guesses() {
        assert_eq!(guess("Terminal"), ("terminal", "slate"));
        assert_eq!(guess("Visual Studio Code"), ("code", "purple"));
        assert_eq!(guess("Google Chrome"), ("browser", "orange"));
        assert_eq!(guess("Notes"), ("star", "sky"));
    }

    #[test]
    fn lists_installed_apps() {
        let launcher = MacLauncher::new(std::env::temp_dir());
        let apps = launcher.apps();
        assert!(
            apps.iter()
                .any(|a| a.bundle == "com.apple.finder" || a.bundle == "com.apple.Safari")
        );
        assert!(apps.iter().all(|a| !a.bundle.is_empty()));
    }

    #[test]
    fn missing_things_are_reported_not_opened() {
        let launcher = MacLauncher::new(std::env::temp_dir());
        let app = Action::App {
            bundle: "invalid.sidedesk.nothing".into(),
            label: "x".into(),
        };
        assert_eq!(launcher.run(&app).unwrap_err().0, "앱을 찾지 못했습니다");
        let file = Action::Open {
            path: "/nonexistent/sidedesk/file.txt".into(),
        };
        assert_eq!(launcher.run(&file).unwrap_err().0, "파일을 찾지 못했습니다");
        let url = Action::Url {
            url: "file:///etc/hosts".into(),
        };
        assert!(launcher.run(&url).is_err());
    }

    #[test]
    fn stopped_launchers_do_not_start_shortcuts() {
        let launcher = MacLauncher::new(std::env::temp_dir());
        launcher.stop();
        assert!(launcher.focus_shortcuts().is_empty());
        let focus = Action::System {
            action: SystemAction::Focus {
                on: "켜기".into(),
                off: "끄기".into(),
            },
        };
        assert!(launcher.run(&focus).is_err());
    }

    #[test]
    fn slow_shortcuts_are_ended_and_stop_ends_running_ones() {
        let focus = Action::System {
            action: SystemAction::Focus {
                on: "5".into(),
                off: "5".into(),
            },
        };
        // /bin/sleep 을 단축어 프로그램 대신 쓴다. 시간 제한을 넘기면 끝내고 실패로 본다.
        let quick = MacLauncher::with_tool(
            std::env::temp_dir(),
            PathBuf::from("/bin/sleep"),
            Duration::from_millis(200),
        );
        let start = Instant::now();
        assert!(quick.shortcuts(&["5"], false).is_some_and(|(ok, _)| !ok));
        // 5초짜리를 기다리지 않았는지만 본다. 느린 CI 서버를 생각해 넉넉히.
        assert!(start.elapsed() < Duration::from_secs(4));
        let launcher = std::sync::Arc::new(MacLauncher::with_tool(
            std::env::temp_dir(),
            PathBuf::from("/bin/sleep"),
            Duration::from_secs(10),
        ));
        let worker = {
            let launcher = std::sync::Arc::clone(&launcher);
            thread::spawn(move || launcher.shortcuts(&["5"], false))
        };
        thread::sleep(Duration::from_millis(200));
        let start = Instant::now();
        launcher.stop();
        let _ = worker.join().unwrap();
        assert!(start.elapsed() < Duration::from_secs(3));
        assert!(launcher.run(&focus).is_err());
    }

    #[test]
    fn focus_state_is_per_pair() {
        let launcher = MacLauncher::new(std::env::temp_dir());
        launcher
            .lock_focus()
            .insert(("업무 켜기".into(), "업무 끄기".into()), true);
        let pair = |on: &str, off: &str| Action::System {
            action: SystemAction::Focus {
                on: on.into(),
                off: off.into(),
            },
        };
        assert_eq!(launcher.toggle(&pair("업무 켜기", "업무 끄기")), Some(true));
        assert_eq!(launcher.toggle(&pair("수면 켜기", "수면 끄기")), None);
    }

    #[test]
    fn apps_can_be_checked_by_bundle() {
        let launcher = MacLauncher::new(std::env::temp_dir());
        assert!(launcher.app_exists("com.apple.finder"));
        assert!(!launcher.app_exists("invalid.sidedesk.nothing"));
    }

    #[test]
    #[ignore = "기본 브라우저로 주소를 연다. 손으로 돌린다"]
    fn opens_a_web_page() {
        let launcher = MacLauncher::new(std::env::temp_dir());
        launcher
            .run(&Action::Url {
                url: "https://example.com".into(),
            })
            .unwrap();
    }

    #[test]
    #[ignore = "단축어 앱 목록 수만 찍는다. 손으로 돌린다"]
    fn counts_shortcuts() {
        let launcher = MacLauncher::new(std::env::temp_dir());
        eprintln!("단축어 {}개", launcher.focus_shortcuts().len());
    }
}
