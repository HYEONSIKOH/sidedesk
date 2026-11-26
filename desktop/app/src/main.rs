// 윈도우에서는 콘솔 창 없이 뜨는 창 프로그램이다. 터미널 명령은 sidedesk-cli 를 쓴다.
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn main() -> std::process::ExitCode {
    sidedesk_app::gui()
}
