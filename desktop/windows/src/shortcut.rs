use sidedesk_domain::{Action, AppChoice, Launcher, PortError, ShortcutHelp};

/// 윈도우 단축키 실행기 자리. SD-8 뒤에 채운다.
pub struct WindowsLauncher;

impl Launcher for WindowsLauncher {
    fn run(&self, _action: &Action) -> Result<(), PortError> {
        Err(PortError::new("윈도우에서는 아직 지원하지 않습니다"))
    }

    fn toggle(&self, _action: &Action) -> Option<bool> {
        None
    }

    fn can_send_keys(&self) -> bool {
        false
    }
}

impl ShortcutHelp for WindowsLauncher {
    fn apps(&self) -> Vec<AppChoice> {
        Vec::new()
    }

    fn pick_path(&self) -> Option<String> {
        None
    }

    fn focus_shortcuts(&self) -> Vec<String> {
        Vec::new()
    }
}
