//! 윈도우 단축키 실행기. 앱은 시작 메뉴의 앱 폴더(`shell:AppsFolder`)에서 고르고 연다.

use std::mem::size_of;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use sidedesk_domain::{Action, AppChoice, Combo, Launcher, PortError, ShortcutHelp, SystemAction};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree};
use windows::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
    KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, SendInput, VIRTUAL_KEY,
};
use windows::Win32::UI::Shell::{
    BHID_EnumItems, FILEOPENDIALOGOPTIONS, FOLDERID_AppsFolder, FOS_FORCEFILESYSTEM,
    FOS_PICKFOLDERS, FileOpenDialog, IEnumShellItems, IFileOpenDialog, IShellItem, KF_FLAG_DEFAULT,
    SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, SHGetKnownFolderItem, SIGDN,
    SIGDN_FILESYSPATH, SIGDN_NORMALDISPLAY, SIGDN_PARENTRELATIVEPARSING, ShellExecuteExW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::PCWSTR;

use crate::com::{Sta, speaker};
use crate::handle::wide;

/// 앱 목록을 다시 읽기 전까지 기억하는 시간
const APPS_FRESH: Duration = Duration::from_secs(30);

#[derive(Default)]
pub struct WindowsLauncher {
    apps: Mutex<Option<(Instant, Vec<AppChoice>)>>,
}

fn failed(what: &str) -> PortError {
    PortError::new(format!("{what} 못했습니다"))
}

impl WindowsLauncher {
    fn app_list(&self) -> Vec<AppChoice> {
        let mut held = self.apps.lock().unwrap_or_else(|lock| lock.into_inner());
        if let Some((at, apps)) = held.as_ref()
            && at.elapsed() < APPS_FRESH
        {
            return apps.clone();
        }
        let apps = read_apps();
        *held = Some((Instant::now(), apps.clone()));
        apps
    }

    fn known(&self, bundle: &str) -> bool {
        self.app_list().iter().any(|app| app.bundle == bundle)
    }
}

impl Launcher for WindowsLauncher {
    fn run(&self, action: &Action) -> Result<(), PortError> {
        match action {
            Action::App { bundle, .. } => {
                // 손으로 고친 설정 파일의 아무 값이나 열지 않게, 지금 앱 목록에 있는 것만 연다.
                if bundle.chars().any(char::is_control) || !self.known(bundle) {
                    return Err(PortError::new("앱을 찾지 못했습니다"));
                }
                shell_open(&app_target(bundle)).map_err(|_| failed("앱을 열지"))
            }
            // 경로와 주소 검사는 domain 이 실행 직전에 한다.
            Action::Open { path } => shell_open(path).map_err(|_| failed("파일을 열지")),
            Action::Url { url } => shell_open(url).map_err(|_| failed("주소를 열지")),
            Action::Keys { combo } => send_combo(combo),
            Action::System { action } => match action {
                SystemAction::Mute => {
                    let volume = speaker().ok_or_else(|| PortError::new("소리 장치가 없습니다"))?;
                    // SAFETY: 살아 있는 볼륨 조절기. 알림 문맥은 없음.
                    unsafe {
                        let muted = volume.GetMute().map_err(|_| failed("소리를 바꾸지"))?;
                        volume
                            .SetMute(!muted.as_bool(), std::ptr::null())
                            .map_err(|_| failed("소리를 바꾸지"))
                    }
                }
                SystemAction::Lock => {
                    // SAFETY: 인자 없는 호출. 성공 값은 잠그기 시작만 뜻한다.
                    if unsafe { windows_sys::Win32::System::Shutdown::LockWorkStation() } == 0 {
                        return Err(failed("화면을 잠그지"));
                    }
                    Ok(())
                }
                SystemAction::Focus { .. } => {
                    Err(PortError::new("윈도우에서는 방해 금지를 지원하지 않습니다"))
                }
            },
        }
    }

    fn toggle(&self, action: &Action) -> Option<bool> {
        match action {
            Action::System {
                action: SystemAction::Mute,
            } => {
                let volume = speaker()?;
                // SAFETY: 살아 있는 볼륨 조절기
                unsafe { volume.GetMute() }
                    .ok()
                    .map(|muted| muted.as_bool())
            }
            _ => None,
        }
    }

    /// 권한이 필요 없다. 관리자 권한 창이 앞이면 윈도우가 조용히 막는데 알 방법이 없다.
    fn can_send_keys(&self) -> bool {
        true
    }
}

impl ShortcutHelp for WindowsLauncher {
    fn apps(&self) -> Vec<AppChoice> {
        self.app_list()
    }

    fn pick_path(&self) -> Option<String> {
        pick(FOS_FORCEFILESYSTEM)
    }

    fn pick_folder(&self) -> Option<String> {
        pick(FOS_FORCEFILESYSTEM | FOS_PICKFOLDERS)
    }

    fn focus_shortcuts(&self) -> Vec<String> {
        Vec::new()
    }

    fn focus_supported(&self) -> bool {
        false
    }

    fn separate_folder_pick(&self) -> bool {
        true
    }

    fn app_exists(&self, bundle: &str) -> bool {
        self.known(bundle)
    }
}

fn app_target(bundle: &str) -> String {
    format!("shell:AppsFolder\\{bundle}")
}

/// 기본 동작(열기)으로 연다. 오류 창을 띄우지 않고, 짧게 사는 스레드라 끝날 때까지 기다린다.
fn shell_open(target: &str) -> windows::core::Result<()> {
    let _sta = Sta::enter();
    let verb = wide("open");
    let file = wide(target);
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(file.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    // SAFETY: 구조체의 글자 포인터는 이 호출 동안 산다.
    unsafe { ShellExecuteExW(&mut info) }
}

/// 셸 항목의 이름. 받은 글자는 우리가 놓는다.
fn display_name(item: &IShellItem, kind: SIGDN) -> Option<String> {
    // SAFETY: 살아 있는 항목. 돌려받은 글자는 CoTaskMemFree 로 놓는다.
    unsafe {
        let raw = item.GetDisplayName(kind).ok()?;
        let text = raw.to_string().ok();
        CoTaskMemFree(Some(raw.0 as *const _));
        text.filter(|t| !t.is_empty())
    }
}

/// 시작 메뉴의 앱(일반 앱과 스토어 앱). 이름 순, 같은 이름은 하나.
fn read_apps() -> Vec<AppChoice> {
    let _sta = Sta::enter();
    let mut apps: Vec<AppChoice> = Vec::new();
    // SAFETY: STA 안에서 셸 폴더를 열고 항목을 하나씩 받는다.
    let listed = unsafe {
        (|| -> windows::core::Result<()> {
            let folder: IShellItem =
                SHGetKnownFolderItem(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None)?;
            let items: IEnumShellItems = folder.BindToHandler(None, &BHID_EnumItems)?;
            loop {
                let mut got = [None];
                let mut fetched = 0u32;
                if items.Next(&mut got, Some(&mut fetched)).is_err() || fetched == 0 {
                    break;
                }
                let Some(item) = got[0].take() else {
                    break;
                };
                if let (Some(name), Some(bundle)) = (
                    display_name(&item, SIGDN_NORMALDISPLAY),
                    display_name(&item, SIGDN_PARENTRELATIVEPARSING),
                ) {
                    let (icon, color) = guess(&name);
                    apps.push(AppChoice {
                        name,
                        bundle,
                        icon: icon.to_owned(),
                        color: color.to_owned(),
                    });
                }
            }
            Ok(())
        })()
    };
    if listed.is_err() {
        return Vec::new();
    }
    // 이름이 같은 두 판(예: x86 과 x64)도 둘 다 고를 수 있게 열 값으로만 중복을 지운다.
    apps.sort_by(|a, b| {
        (a.name.to_lowercase(), &a.bundle).cmp(&(b.name.to_lowercase(), &b.bundle))
    });
    apps.dedup_by(|a, b| a.bundle == b.bundle);
    apps
}

/// 운영체제 고르기 창. UI 스레드(이벤트 루프)에서 부른다. 고르지 않으면 없음.
/// 지금 앞에 있는 이 스레드의 창(설정 창)을 주인으로 띄워 설정 창 뒤로 숨지 않고, 열린 동안 설정 창을 막는다.
fn pick(options: FILEOPENDIALOGOPTIONS) -> Option<String> {
    let _sta = Sta::enter();
    // SAFETY: STA 안에서 고르기 창을 만들고 모달로 띄운다. 주인 창은 이 스레드의 활성 창(없으면 없음).
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let now = dialog.GetOptions().ok()?;
        dialog.SetOptions(now | options).ok()?;
        let owner = GetActiveWindow();
        dialog.Show((!owner.is_invalid()).then_some(owner)).ok()?;
        let item = dialog.GetResult().ok()?;
        display_name(&item, SIGDN_FILESYSPATH)
    }
}

/// 앱 이름으로 그림과 색을 짐작한다(맥 표에 윈도우 앱 이름을 더함).
pub fn guess(name: &str) -> (&'static str, &'static str) {
    let lower = name.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| lower.contains(w));
    if has(&[
        "terminal",
        "powershell",
        "command prompt",
        "명령 프롬프트",
        "터미널",
    ]) {
        ("terminal", "slate")
    } else if has(&["file explorer", "파일 탐색기", "explorer"]) {
        ("folder", "green")
    } else if has(&[
        "code",
        "visual studio",
        "intellij",
        "cursor",
        "sublime",
        "notepad++",
    ]) {
        ("code", "purple")
    } else if has(&[
        "chrome", "firefox", "edge", "brave", "opera", "browser", "whale",
    ]) {
        ("browser", "orange")
    } else if has(&["calculator", "계산기"]) {
        ("calculator", "teal")
    } else if has(&["settings", "control panel", "설정", "제어판"]) {
        ("settings", "gray")
    } else if has(&["task manager", "작업 관리자", "resource monitor"]) {
        ("activity", "olive")
    } else if has(&[
        "music",
        "spotify",
        "media player",
        "미디어 플레이어",
        "음악",
    ]) {
        ("music", "rose")
    } else {
        ("star", "sky")
    }
}

/// 키 하나를 보내는 방법
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Key {
    /// 가상 키, 확장 키 여부(화살표, Home 같은 키는 숫자 패드와 구분하려면 확장 표시가 필요)
    Virtual(u16, bool),
    /// 자판 배열과 상관없는 물리 자리(US 배열의 스캔 코드)
    Scan(u16),
}

/// 브라우저 키 이름(`KeyboardEvent.code`)을 윈도우 키로. 모르면 없음.
fn key_of(code: &str) -> Option<Key> {
    if let Some(letter) = code.strip_prefix("Key")
        && let [c] = letter.as_bytes()
        && c.is_ascii_uppercase()
    {
        return Some(Key::Virtual(u16::from(*c), false));
    }
    if let Some(digit) = code.strip_prefix("Digit")
        && let [c] = digit.as_bytes()
        && c.is_ascii_digit()
    {
        return Some(Key::Virtual(u16::from(*c), false));
    }
    if let Some(n) = code.strip_prefix('F').and_then(|n| n.parse::<u16>().ok())
        && (1..=12).contains(&n)
    {
        return Some(Key::Virtual(0x70 + n - 1, false));
    }
    let key = match code {
        "Space" => Key::Virtual(0x20, false),
        "Enter" => Key::Virtual(0x0D, false),
        "Tab" => Key::Virtual(0x09, false),
        "Escape" => Key::Virtual(0x1B, false),
        "Backspace" => Key::Virtual(0x08, false),
        "Delete" => Key::Virtual(0x2E, true),
        "Home" => Key::Virtual(0x24, true),
        "End" => Key::Virtual(0x23, true),
        "PageUp" => Key::Virtual(0x21, true),
        "PageDown" => Key::Virtual(0x22, true),
        "ArrowLeft" => Key::Virtual(0x25, true),
        "ArrowUp" => Key::Virtual(0x26, true),
        "ArrowRight" => Key::Virtual(0x27, true),
        "ArrowDown" => Key::Virtual(0x28, true),
        "Minus" => Key::Scan(0x0C),
        "Equal" => Key::Scan(0x0D),
        "BracketLeft" => Key::Scan(0x1A),
        "BracketRight" => Key::Scan(0x1B),
        "Backslash" => Key::Scan(0x2B),
        "Semicolon" => Key::Scan(0x27),
        "Quote" => Key::Scan(0x28),
        "Backquote" => Key::Scan(0x29),
        "Comma" => Key::Scan(0x33),
        "Period" => Key::Scan(0x34),
        "Slash" => Key::Scan(0x35),
        "IntlBackslash" => Key::Scan(0x56),
        _ => return None,
    };
    Some(key)
}

const VK_LWIN: u16 = 0x5B;
const VK_CONTROL: u16 = 0x11;
const VK_MENU: u16 = 0x12;
const VK_SHIFT: u16 = 0x10;

/// 누를 조합키(Win, Ctrl, Alt, Shift 순)와 키
fn strokes(combo: &Combo) -> Option<(Vec<Key>, Key)> {
    let key = key_of(&combo.code)?;
    let mut held = Vec::new();
    for (on, vk) in [
        (combo.command, VK_LWIN),
        (combo.control, VK_CONTROL),
        (combo.option, VK_MENU),
        (combo.shift, VK_SHIFT),
    ] {
        if on {
            held.push(Key::Virtual(vk, vk == VK_LWIN));
        }
    }
    Some((held, key))
}

fn input(key: Key, up: bool) -> INPUT {
    let release = if up {
        KEYEVENTF_KEYUP
    } else {
        KEYBD_EVENT_FLAGS(0)
    };
    let (vk, scan, flags) = match key {
        Key::Virtual(vk, true) => (vk, 0, release | KEYEVENTF_EXTENDEDKEY),
        Key::Virtual(vk, false) => (vk, 0, release),
        Key::Scan(scan) => (0, scan, release | KEYEVENTF_SCANCODE),
    };
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send(inputs: &[INPUT]) -> usize {
    // SAFETY: 우리 배열과 그 원소 크기를 그대로 넘긴다.
    unsafe { SendInput(inputs, size_of::<INPUT>() as i32) as usize }
}

/// 누르기, 키, 떼기를 한 번에 보내 사이에 다른 입력이 끼지 않게 한다.
/// 덜 들어가면 떼기만 다시 보내 눌린 채 남지 않게 한다.
fn send_combo(combo: &Combo) -> Result<(), PortError> {
    let (held, key) = strokes(combo).ok_or_else(|| PortError::new("보낼 수 없는 키입니다"))?;
    let mut all: Vec<INPUT> = held.iter().map(|k| input(*k, false)).collect();
    all.push(input(key, false));
    all.push(input(key, true));
    let releases: Vec<INPUT> = held.iter().rev().map(|k| input(*k, true)).collect();
    all.extend(releases.iter().copied());
    // 앞쪽 일부만 들어갔을 때(예: Win 누름만) 떼기를 다시 보내면 Win 떼기로 시작 메뉴가 열릴 수 있다.
    // 드문 일이고, 눌린 채 남는 것보다 낫다.
    if send(&all) < all.len() {
        let mut ups = vec![input(key, true)];
        ups.extend(releases);
        send(&ups);
        return Err(PortError::new("키를 보내지 못했습니다"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn combo(code: &str) -> Combo {
        Combo {
            code: code.to_owned(),
            command: false,
            control: true,
            option: false,
            shift: true,
        }
    }

    #[test]
    fn every_allowed_code_has_a_key() {
        for code in sidedesk_domain::KEY_CODES {
            assert!(key_of(code).is_some(), "{code}");
        }
        assert_eq!(key_of("KeyA"), Some(Key::Virtual(0x41, false)));
        assert_eq!(key_of("F5"), Some(Key::Virtual(0x74, false)));
        assert_eq!(key_of("ArrowUp"), Some(Key::Virtual(0x26, true)));
        assert_eq!(key_of("Minus"), Some(Key::Scan(0x0C)));
        for wrong in ["Keya", "KeyAB", "F13", "F0", "Insert", ""] {
            assert_eq!(key_of(wrong), None, "{wrong}");
        }
    }

    #[test]
    fn modifiers_go_down_in_order() {
        let (held, key) = strokes(&combo("KeyS")).unwrap();
        assert_eq!(
            held,
            [
                Key::Virtual(VK_CONTROL, false),
                Key::Virtual(VK_SHIFT, false)
            ]
        );
        assert_eq!(key, Key::Virtual(0x53, false));
        let mut all = combo("KeyS");
        all.command = true;
        all.option = true;
        let (held, _) = strokes(&all).unwrap();
        assert_eq!(held[0], Key::Virtual(VK_LWIN, true));
        assert_eq!(held.len(), 4);
    }

    #[test]
    fn apps_open_through_the_apps_folder() {
        assert_eq!(
            app_target("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"),
            "shell:AppsFolder\\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"
        );
        assert_eq!(guess("Windows Terminal"), ("terminal", "slate"));
        assert_eq!(guess("계산기"), ("calculator", "teal"));
    }

    #[test]
    fn focus_is_not_offered() {
        let launcher = WindowsLauncher::default();
        assert!(!launcher.focus_supported());
        assert!(launcher.separate_folder_pick());
        assert!(launcher.focus_shortcuts().is_empty());
        let focus = Action::System {
            action: SystemAction::Focus {
                on: "a".into(),
                off: "b".into(),
            },
        };
        assert!(launcher.run(&focus).is_err());
    }

    #[test]
    #[ignore = "시작 메뉴 앱 폴더를 읽는다. CI 윈도우 작업에서 돌린다"]
    fn windows_only_lists_start_menu_apps() {
        let launcher = WindowsLauncher::default();
        let apps = launcher.apps();
        eprintln!(
            "앱 {}개, 앞 다섯: {:?}",
            apps.len(),
            &apps[..apps.len().min(5)]
        );
        assert!(!apps.is_empty());
        assert!(
            apps.iter()
                .all(|a| !a.name.is_empty() && !a.bundle.is_empty())
        );
        assert!(launcher.app_exists(&apps[0].bundle));
        assert!(!launcher.app_exists("없는.앱!App"));
        let refused = launcher.run(&Action::App {
            bundle: "없는.앱!App".into(),
            label: String::new(),
        });
        assert_eq!(refused.unwrap_err().0, "앱을 찾지 못했습니다");
    }

    #[test]
    #[ignore = "화면 세션에 키를 보낸다. CI 윈도우 작업에서 돌린다"]
    fn windows_only_sends_a_key_press_and_release() {
        // Shift 하나를 누르고 뗀다(어느 창에도 글자가 들어가지 않음).
        let shift = Key::Virtual(VK_SHIFT, false);
        assert_eq!(send(&[input(shift, false), input(shift, true)]), 2);
    }

    #[test]
    #[ignore = "기본 출력 장치를 쓴다. CI 윈도우 작업에서 돌린다"]
    fn windows_only_mute_flips_and_comes_back_when_there_is_a_device() {
        let launcher = WindowsLauncher::default();
        let mute = Action::System {
            action: SystemAction::Mute,
        };
        let Some(before) = launcher.toggle(&mute) else {
            eprintln!("소리 장치 없음");
            assert!(launcher.run(&mute).is_err());
            return;
        };
        launcher.run(&mute).unwrap();
        assert_eq!(launcher.toggle(&mute), Some(!before));
        launcher.run(&mute).unwrap();
        assert_eq!(launcher.toggle(&mute), Some(before));
    }
}
