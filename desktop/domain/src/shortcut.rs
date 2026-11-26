//! 단축키: 모양, 등록 검사, 포트. 폰은 번호만 보내고 등록 내용은 컴퓨터에만 있다.

use serde::{Deserialize, Serialize};

use crate::port::PortError;

/// 폰과 설정 창이 함께 쓰는 그림 이름표
pub const ICONS: [&str; 14] = [
    "terminal",
    "folder",
    "code",
    "browser",
    "calculator",
    "settings",
    "activity",
    "music",
    "volume",
    "dnd",
    "lock",
    "file",
    "star",
    "bolt",
];
pub const COLORS: [&str; 10] = [
    "slate", "green", "purple", "orange", "teal", "gray", "olive", "rose", "sand", "sky",
];
pub const NAME_LEN: usize = 40;
pub const PATH_LEN: usize = 4096;
pub const URL_LEN: usize = 2048;

/// 키 조합이 받는 키(브라우저 `KeyboardEvent.code` 이름). 운영체제가 저마다 키 번호로 바꾼다.
pub const KEY_CODES: [&str; 74] = [
    "KeyA",
    "KeyB",
    "KeyC",
    "KeyD",
    "KeyE",
    "KeyF",
    "KeyG",
    "KeyH",
    "KeyI",
    "KeyJ",
    "KeyK",
    "KeyL",
    "KeyM",
    "KeyN",
    "KeyO",
    "KeyP",
    "KeyQ",
    "KeyR",
    "KeyS",
    "KeyT",
    "KeyU",
    "KeyV",
    "KeyW",
    "KeyX",
    "KeyY",
    "KeyZ",
    "Digit0",
    "Digit1",
    "Digit2",
    "Digit3",
    "Digit4",
    "Digit5",
    "Digit6",
    "Digit7",
    "Digit8",
    "Digit9",
    "F1",
    "F2",
    "F3",
    "F4",
    "F5",
    "F6",
    "F7",
    "F8",
    "F9",
    "F10",
    "F11",
    "F12",
    "ArrowLeft",
    "ArrowRight",
    "ArrowUp",
    "ArrowDown",
    "Space",
    "Enter",
    "Tab",
    "Escape",
    "Backspace",
    "Delete",
    "Home",
    "End",
    "PageUp",
    "PageDown",
    "Minus",
    "Equal",
    "BracketLeft",
    "BracketRight",
    "Backslash",
    "Semicolon",
    "Quote",
    "Comma",
    "Period",
    "Slash",
    "Backquote",
    "IntlBackslash",
];

/// 파일 열기로 받지 않는 확장자(기본 앱이 열면서 무언가를 실행하거나 다른 주소로 넘김)
const RUNNABLE: [&str; 23] = [
    "app",
    "command",
    "sh",
    "tool",
    "pkg",
    "terminal",
    "workflow",
    "action",
    "scpt",
    "applescript",
    "jar",
    "webloc",
    "inetloc",
    "fileloc",
    "url",
    "prefpane",
    "mpkg",
    "mobileconfig",
    "scptd",
    "osax",
    "service",
    "wflow",
    "shortcut",
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Combo {
    pub code: String,
    #[serde(default)]
    pub command: bool,
    #[serde(default)]
    pub control: bool,
    #[serde(default)]
    pub option: bool,
    #[serde(default)]
    pub shift: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SystemAction {
    Mute,
    Lock,
    /// 단축어 앱의 단축어 이름 둘(켜기, 끄기)
    Focus {
        on: String,
        off: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    /// 맥은 묶음 ID, label 은 보일 앱 이름
    App {
        bundle: String,
        label: String,
    },
    Open {
        path: String,
    },
    Url {
        url: String,
    },
    Keys {
        combo: Combo,
    },
    System {
        action: SystemAction,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shortcut {
    pub id: u32,
    pub name: String,
    pub icon: String,
    pub color: String,
    pub action: Action,
}

/// 보관하는 전부
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Saved {
    /// 이 컴퓨터의 단축키 장부 번호. 되돌리면 새로 정해 옛 번호의 명령을 막는다.
    pub book: u64,
    /// 다음에 줄 번호. 지운 번호를 다시 쓰지 않는다.
    pub next_id: u32,
    pub items: Vec<Shortcut>,
}

impl Saved {
    pub fn new(book: u64) -> Self {
        Self {
            book,
            next_id: 1,
            items: Vec::new(),
        }
    }

    /// 손으로 고친 파일을 바로잡는다: 같은 번호는 뒤엣것을 버리고, 다음 번호는 가장 큰 번호 뒤로.
    /// 버린 것이 있으면 true.
    pub fn repair(&mut self) -> bool {
        let before = self.items.len();
        let mut seen = std::collections::HashSet::new();
        self.items.retain(|item| seen.insert(item.id));
        let top = self.items.iter().map(|item| item.id).max().unwrap_or(0);
        self.next_id = self.next_id.max(top.saturating_add(1)).max(1);
        before != self.items.len()
    }
}

impl Action {
    /// 등록과 실행 직전에 같이 본다. 실패하면 사람이 읽을 까닭.
    pub fn check(&self) -> Result<(), &'static str> {
        match self {
            Self::App { bundle, label } => {
                if bundle.is_empty() || bundle.len() > PATH_LEN || label.len() > PATH_LEN {
                    return Err("앱을 고르세요");
                }
                Ok(())
            }
            Self::Open { path } => check_path(path),
            Self::Url { url } => check_url(url),
            Self::Keys { combo } => {
                if KEY_CODES.contains(&combo.code.as_str()) {
                    Ok(())
                } else {
                    Err("보낼 수 없는 키입니다")
                }
            }
            Self::System {
                action: SystemAction::Focus { on, off },
            } => {
                for name in [on, off] {
                    if name.trim().is_empty() || name.starts_with('-') || name.len() > NAME_LEN * 4
                    {
                        return Err("방해 금지 단축어를 고르세요");
                    }
                }
                Ok(())
            }
            Self::System { .. } => Ok(()),
        }
    }

    pub fn is_system(&self) -> bool {
        matches!(self, Self::System { .. })
    }
}

impl Shortcut {
    /// 이름, 이름표 검사. 모르는 이름표는 기본값으로 바꾼다.
    pub fn tidy(&mut self) -> Result<(), &'static str> {
        self.name = self
            .name
            .trim()
            .chars()
            .filter(|c| !c.is_control())
            .collect();
        if self.name.chars().count() > NAME_LEN {
            return Err("이름은 40글자까지입니다");
        }
        if !ICONS.contains(&self.icon.as_str()) {
            self.icon = "star".to_owned();
        }
        if !COLORS.contains(&self.color.as_str()) {
            self.color = "gray".to_owned();
        }
        self.action.check()
    }
}

/// http, https 이고 호스트가 있는 주소만
pub fn check_url(url: &str) -> Result<(), &'static str> {
    const WRONG: &str = "http 나 https 로 시작하는 주소만 열 수 있습니다";
    if url.is_empty() || url.len() > URL_LEN {
        return Err(WRONG);
    }
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(WRONG);
    }
    let Some((scheme, rest)) = url.split_once(':') else {
        return Err(WRONG);
    };
    if !matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https") {
        return Err(WRONG);
    }
    let Some(rest) = rest.strip_prefix("//") else {
        return Err(WRONG);
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host.is_empty() || host.starts_with('@') {
        return Err(WRONG);
    }
    Ok(())
}

/// 윈도우에서 기본 앱이 열면서 무언가를 실행하는 확장자. `PATHEXT` 값도 더해 본다.
// 윈도우에서만 쓰지만 글자 규칙이라 시험은 어디서나 돈다.
#[cfg_attr(not(windows), allow(dead_code))]
const WINDOWS_RUNNABLE: [&str; 39] = [
    "exe",
    "com",
    "bat",
    "cmd",
    "ps1",
    "psm1",
    "psd1",
    "msi",
    "msp",
    "mst",
    "lnk",
    "url",
    "pif",
    "scr",
    "cpl",
    "msc",
    "hta",
    "js",
    "jse",
    "vbs",
    "vbe",
    "wsf",
    "wsh",
    "reg",
    "inf",
    "jar",
    "appref-ms",
    "application",
    "settingcontent-ms",
    "library-ms",
    "search-ms",
    "diagcab",
    "appx",
    "appxbundle",
    "msix",
    "msixbundle",
    "appinstaller",
    "scf",
    "gadget",
];

/// 윈도우 경로 규칙. 문제가 있으면 그 까닭. 운영체제와 상관없는 글자 검사라 어디서나 시험한다.
/// - UNC(`\\host\share`), 장치 경로(`\\?\`, `\\.\`)는 받지 않는다(다른 컴퓨터에 로그인 정보가 새어 나감).
/// - 드라이브 글자 뒤가 아닌 `:` 은 대체 데이터 스트림이라 받지 않는다.
/// - 끝의 점과 공백은 윈도우가 지우므로 지운 뒤 확장자를 본다.
// 윈도우에서만 쓰지만 글자 규칙이라 시험은 어디서나 돈다.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn windows_path_problem(path: &str, pathext: &str) -> Option<&'static str> {
    let slashed = path.replace('/', "\\");
    // 받는 꼴은 드라이브 글자로 시작하는 전체 경로(`C:\..`) 하나. 나머지(UNC, `\\?\`, `\??\`, 장치 경로,
    // 상대 경로)는 모두 받지 않는다. 연결된 네트워크 드라이브(`Z:`)는 드라이브 글자라 받는다.
    let rest = match slashed.as_bytes() {
        [drive, b':', b'\\', ..] if drive.is_ascii_alphabetic() => &slashed[2..],
        [b'\\', ..] => return Some("네트워크 경로와 장치 경로는 열 수 없습니다"),
        _ => return Some("드라이브부터 시작하는 전체 경로를 고르세요"),
    };
    if rest.contains(':') {
        return Some("이 경로는 열 수 없습니다");
    }
    windows_runnable(&slashed, pathext).then_some("실행 파일은 앱 열기로 등록하세요")
}

// 윈도우에서만 쓰지만 글자 규칙이라 시험은 어디서나 돈다.
#[cfg_attr(not(windows), allow(dead_code))]
fn windows_runnable(path: &str, pathext: &str) -> bool {
    let name = path.rsplit('\\').next().unwrap_or("");
    let name = name.trim_end_matches(['.', ' ']);
    let Some((_, extension)) = name.rsplit_once('.') else {
        return false;
    };
    let extension = extension.to_ascii_lowercase();
    WINDOWS_RUNNABLE.contains(&extension.as_str())
        || pathext
            .split(';')
            .filter_map(|e| e.trim().strip_prefix('.'))
            .any(|e| e.eq_ignore_ascii_case(&extension))
}

/// 실행 파일과 바로 가기 파일은 "앱 열기"로만. 심볼릭 링크는 실제 대상으로 본다.
pub fn check_path(path: &str) -> Result<(), &'static str> {
    if path.is_empty() || path.len() > PATH_LEN || path.chars().any(char::is_control) {
        return Err("파일이나 폴더를 고르세요");
    }
    // 윈도우는 파일을 열어 보기(canonicalize) 전에 글자로 먼저 거른다. 네트워크 경로를 열면
    // 그것만으로 다른 컴퓨터에 로그인 정보(NTLM)가 나간다.
    #[cfg(windows)]
    let pathext = std::env::var("PATHEXT").unwrap_or_default();
    #[cfg(windows)]
    if let Some(problem) = windows_path_problem(path, &pathext) {
        return Err(problem);
    }
    let real = std::fs::canonicalize(path).unwrap_or_else(|_| path.into());
    #[cfg(windows)]
    {
        // canonicalize 는 `\\?\C:\..` 모양을 돌려준다. 링크가 가리키는 실제 파일이 네트워크 공유이거나
        // 실행 파일이면 받지 않는다(짧은 8.3 이름도 여기서 긴 이름으로 풀린다).
        let real = real.to_string_lossy();
        if real.starts_with("\\\\?\\UNC\\") {
            return Err("네트워크 경로와 장치 경로는 열 수 없습니다");
        }
        let real = real.strip_prefix("\\\\?\\").unwrap_or(&real);
        if windows_runnable(real, &pathext) {
            return Err("실행 파일은 앱 열기로 등록하세요");
        }
    }
    let runnable = |p: &std::path::Path| {
        p.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| RUNNABLE.contains(&e.to_ascii_lowercase().as_str()))
    };
    // 실행 비트는 확장자가 없을 때만 본다(exFAT, FAT 저장 장치의 문서는 모두 실행 비트가 있다).
    let bare = real.extension().is_none();
    if runnable(std::path::Path::new(path)) || runnable(&real) || (bare && executable(&real)) {
        return Err("실행 파일은 앱 열기로 등록하세요");
    }
    Ok(())
}

#[cfg(unix)]
fn executable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// 윈도우는 확장자 규칙(`windows_path_problem`)으로 따로 거른다.
#[cfg(not(unix))]
fn executable(_path: &std::path::Path) -> bool {
    false
}

/// 단축키 보관(설정 폴더의 파일)
pub trait ShortcutStore: Send + Sync {
    /// 깨졌으면 직전 정상본, 그것도 없으면 빈 목록. 되돌렸으면 알림 문장도.
    fn load(&self) -> Result<(Saved, Option<String>), PortError>;
    fn save(&self, saved: &Saved) -> Result<(), PortError>;
}

/// 실제 실행
pub trait Launcher: Send + Sync {
    fn run(&self, action: &Action) -> Result<(), PortError>;
    /// 켜짐 상태가 있는 동작의 지금 상태(소리 끄기는 꺼짐이 true). 모르면 없음.
    fn toggle(&self, action: &Action) -> Option<bool>;
    /// 키 조합을 보낼 수 있는지(맥 손쉬운 사용 권한)
    fn can_send_keys(&self) -> bool;
    /// 돌고 있는 외부 프로그램을 끝낸다.
    fn stop(&self) {}
}

/// 설정 창에서 고를 앱
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppChoice {
    pub name: String,
    pub bundle: String,
    pub icon: String,
    pub color: String,
}

/// 설정 창 도우미(운영체제마다)
pub trait ShortcutHelp: Send + Sync {
    fn apps(&self) -> Vec<AppChoice>;
    /// 운영체제 파일 고르기 창. 메인 스레드에서 부른다.
    fn pick_path(&self) -> Option<String>;
    /// 단축어 앱의 단축어 이름
    fn focus_shortcuts(&self) -> Vec<String>;
    /// 묶음 ID 의 앱이 아직 있는지
    fn app_exists(&self, _bundle: &str) -> bool {
        true
    }
    /// 방해 금지(단축어)를 고를 수 있는지. 윈도우는 공개 방법이 없어 숨긴다.
    fn focus_supported(&self) -> bool {
        true
    }
    /// 폴더 고르기를 따로 두는지(윈도우 창은 파일과 폴더를 한 번에 못 고름)
    fn separate_folder_pick(&self) -> bool {
        false
    }
    /// 폴더 고르기 창. 메인 스레드에서 부른다.
    fn pick_folder(&self) -> Option<String> {
        None
    }
    /// 손쉬운 사용 설정을 연다.
    fn open_key_settings(&self) {}
    /// 자동화(다른 앱 제어) 설정을 연다.
    fn open_automation_settings(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_paths_that_run_or_leak_are_refused() {
        let pathext = ".COM;.EXE;.BAT;.CMD;.VBS;.JS;.WS;.MSC;.PY";
        for bad in [
            r"C:\x\a.bat",
            r"C:\x\a.PS1",
            r"C:\x\a.lnk",
            r"C:\x\a.hta",
            r"C:\x\a.exe.",
            r"C:\x\a.cmd  ",
            r"C:\x\run.py",
            r"C:\x\a.settingcontent-ms",
        ] {
            assert_eq!(
                windows_path_problem(bad, pathext),
                Some("실행 파일은 앱 열기로 등록하세요"),
                "{bad}"
            );
        }
        for leak in [
            r"\\server\share\a.txt",
            "//server/share/a.txt",
            r"\\?\C:\x\a.txt",
            r"\\.\PhysicalDrive0",
            r"\\?\UNC\server\share",
        ] {
            assert_eq!(
                windows_path_problem(leak, pathext),
                Some("네트워크 경로와 장치 경로는 열 수 없습니다"),
                "{leak}"
            );
        }
        assert!(windows_path_problem(r"C:\x\a.txt:hidden.exe", pathext).is_some());
        assert!(windows_path_problem(r"C:\x\a.txt:stream", pathext).is_some());
        for odd in [r"\??\UNC\host\share\a.txt", r"\??\C:\a.txt"] {
            assert_eq!(
                windows_path_problem(odd, pathext),
                Some("네트워크 경로와 장치 경로는 열 수 없습니다"),
                "{odd}"
            );
        }
        for relative in ["a.txt", r"x\a.txt", "C:a.txt", r"C:"] {
            assert_eq!(
                windows_path_problem(relative, pathext),
                Some("드라이브부터 시작하는 전체 경로를 고르세요"),
                "{relative}"
            );
        }
        for good in [
            r"C:\x\plan.txt",
            r"D:\사진\a.JPG",
            r"C:\x\folder",
            "C:/x/a.pdf",
        ] {
            assert_eq!(windows_path_problem(good, pathext), None, "{good}");
        }
    }

    #[test]
    fn urls() {
        for good in ["https://example.com", "HTTPS://x", "http://a.b/c?d#e"] {
            assert_eq!(check_url(good), Ok(()), "{good}");
        }
        for bad in [
            "http:/x",
            "javascript:alert(1)",
            "file:///etc/passwd",
            " https://x",
            "https://",
            "https:///path",
            "ftp://x",
            "https://x y",
            "https://@x",
            "",
        ] {
            assert!(check_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn runnable_files_are_refused() {
        let dir = std::env::temp_dir().join(format!("sidedesk-paths-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let plain = dir.join("note.txt");
        std::fs::write(&plain, b"x").unwrap();
        assert_eq!(check_path(plain.to_str().unwrap()), Ok(()));
        assert_eq!(check_path(dir.to_str().unwrap()), Ok(()));
        for name in [
            "a.APP",
            "b.command",
            "c.webloc",
            "d.Terminal",
            "e.fileloc",
            "f.mobileconfig",
            "g.shortcut",
        ] {
            let path = dir.join(name);
            std::fs::write(&path, b"x").unwrap();
            assert!(check_path(path.to_str().unwrap()).is_err(), "{name}");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let script = dir.join("run");
            std::fs::write(&script, b"#!/bin/sh").unwrap();
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
            assert!(check_path(script.to_str().unwrap()).is_err());
            // 링크는 실제 대상으로 본다.
            let link = dir.join("innocent.txt");
            std::os::unix::fs::symlink(&script, &link).unwrap();
            assert!(check_path(link.to_str().unwrap()).is_err());
            // 확장자가 있는 문서는 실행 비트가 있어도 받는다(USB 저장 장치).
            let document = dir.join("report.pdf");
            std::fs::write(&document, b"x").unwrap();
            std::fs::set_permissions(&document, std::fs::Permissions::from_mode(0o777)).unwrap();
            assert_eq!(check_path(document.to_str().unwrap()), Ok(()));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn names_labels_keys_and_focus() {
        let mut s = Shortcut {
            id: 1,
            name: " 이름\n ".into(),
            icon: "unknown".into(),
            color: "neon".into(),
            action: Action::Keys {
                combo: Combo {
                    code: "KeyK".into(),
                    command: true,
                    control: false,
                    option: false,
                    shift: false,
                },
            },
        };
        assert_eq!(s.tidy(), Ok(()));
        assert_eq!(
            (s.name.as_str(), s.icon.as_str(), s.color.as_str()),
            ("이름", "star", "gray")
        );
        s.name = "가".repeat(41);
        assert!(s.tidy().is_err());
        let keys = |code: &str| Action::Keys {
            combo: Combo {
                code: code.into(),
                command: false,
                control: false,
                option: false,
                shift: false,
            },
        };
        assert!(keys("F13").check().is_err());
        assert!(keys("Digit7").check().is_ok());
        let focus = |on: &str| Action::System {
            action: SystemAction::Focus {
                on: on.into(),
                off: "끄기".into(),
            },
        };
        assert!(focus("--output-path").check().is_err());
        assert!(focus("켜기").check().is_ok());
    }

    #[test]
    fn repair_keeps_ids_unique_and_moving_forward() {
        let item = |id| Shortcut {
            id,
            name: String::new(),
            icon: "star".into(),
            color: "gray".into(),
            action: Action::System {
                action: SystemAction::Lock,
            },
        };
        let mut saved = Saved {
            book: 1,
            next_id: 2,
            items: vec![item(5), item(3), item(5)],
        };
        assert!(saved.repair());
        assert_eq!(saved.items.len(), 2);
        assert_eq!(saved.next_id, 6);
        let json = serde_json::to_string(&saved).unwrap();
        assert_eq!(serde_json::from_str::<Saved>(&json).unwrap(), saved);
    }

    /// 윈도우에서 실제 파일로 본다. 네트워크 경로는 열어 보기 전에 거르므로 바로 돌아온다.
    #[cfg(windows)]
    #[test]
    fn windows_real_files_and_network_paths() {
        let start = std::time::Instant::now();
        assert_eq!(
            check_path(r"\\10.255.255.1\share\a.txt"),
            Err("네트워크 경로와 장치 경로는 열 수 없습니다")
        );
        assert!(
            start.elapsed() < std::time::Duration::from_secs(1),
            "네트워크에 나가 기다림: {:?}",
            start.elapsed()
        );
        let folder = std::env::temp_dir().join(format!("sidedesk-paths-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let file = |name: &str| {
            let path = folder.join(name);
            std::fs::write(&path, b"x").unwrap();
            path
        };
        let text = file("plan.txt");
        assert_eq!(check_path(text.to_str().unwrap()), Ok(()));
        for runnable in ["run.bat", "go.lnk", "page.hta"] {
            let path = file(runnable);
            assert_eq!(
                check_path(path.to_str().unwrap()),
                Err("실행 파일은 앱 열기로 등록하세요"),
                "{runnable}"
            );
        }
        // 짧은 8.3 이름도 실제 이름으로 풀어 거른다(그 드라이브가 짧은 이름을 만들 때만).
        let long = file("longer-script-name.cmd");
        let wide: Vec<u16> = long.to_str().unwrap().encode_utf16().chain([0]).collect();
        let mut short = vec![0u16; 1024];
        // SAFETY: 널 끝 경로와 그 길이의 버퍼
        let length = unsafe {
            windows_sys::Win32::Storage::FileSystem::GetShortPathNameW(
                wide.as_ptr(),
                short.as_mut_ptr(),
                short.len() as u32,
            )
        } as usize;
        let short = String::from_utf16_lossy(&short[..length.min(short.len())]);
        eprintln!("짧은 이름: {short}");
        if length > 0 && short != long.to_str().unwrap() {
            assert_eq!(check_path(&short), Err("실행 파일은 앱 열기로 등록하세요"));
        }
        // 심볼릭 링크는 가리키는 실제 파일로 본다(개발자 모드나 관리자 권한이 없으면 건너뜀).
        let link = folder.join("notes.txt");
        if std::os::windows::fs::symlink_file(folder.join("run.bat"), &link).is_ok() {
            assert_eq!(
                check_path(link.to_str().unwrap()),
                Err("실행 파일은 앱 열기로 등록하세요")
            );
        } else {
            eprintln!("심볼릭 링크를 만들 수 없어 건너뜀");
        }
        let _ = std::fs::remove_dir_all(&folder);
    }
}
