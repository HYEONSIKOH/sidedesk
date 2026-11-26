use std::borrow::Cow;
use std::path::{Path, PathBuf};

use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoop, EventLoopBuilder};
#[cfg(target_os = "macos")]
use tao::platform::macos::{WindowBuilderExtMacOS, WindowExtMacOS};
use tao::window::{Theme, Window, WindowBuilder};
use wry::http::{Response, header::CONTENT_TYPE};
use wry::{WebContext, WebView, WebViewBuilder};

use crate::board::Board;

const SCHEME: &str = "sidedesk";
#[cfg(target_os = "windows")]
const START: &str = "http://sidedesk.localhost/";
#[cfg(not(target_os = "windows"))]
const START: &str = "sidedesk://localhost/";

fn asset(path: &str) -> Option<(&'static str, &'static [u8])> {
    match path {
        "/" | "/index.html" => Some((
            "text/html; charset=utf-8",
            include_bytes!("../ui/index.html"),
        )),
        "/app.css" => Some(("text/css; charset=utf-8", include_bytes!("../ui/app.css"))),
        "/app.js" => Some((
            "application/javascript; charset=utf-8",
            include_bytes!("../ui/app.js"),
        )),
        _ => None,
    }
}

/// 빈 도구 막대를 달아 맥 창 제목 영역을 52 높이로 늘린다. 창 버튼이 그 세로 가운데에 온다.
#[cfg(target_os = "macos")]
fn unified_title(window: &tao::window::Window) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSToolbar, NSWindow, NSWindowToolbarStyle};
    let Some(main) = MainThreadMarker::new() else {
        return;
    };
    // tao 가 만든 NSWindow 이고 창이 살아 있는 동안만 쓴다.
    let ns_window = unsafe { &*(window.ns_window() as *const NSWindow) };
    let toolbar = NSToolbar::new(main);
    ns_window.setToolbar(Some(&toolbar));
    ns_window.setToolbarStyle(NSWindowToolbarStyle::Unified);
}

enum Change {
    Render(String),
    /// 화면에서 그대로 돌릴 글(단축키 칸 그리기)
    Script(String),
    Drag,
    Mode(&'static str),
    /// 시험이 화면에서 읽은 글자
    #[cfg(all(test, target_os = "windows"))]
    Read(String),
}

/// 화면 모드. 자동은 컴퓨터 설정을 따른다.
fn mode(text: &str) -> Option<&'static str> {
    match text.trim() {
        "auto" => Some("auto"),
        "light" => Some("light"),
        "dark" => Some("dark"),
        _ => None,
    }
}

fn theme(mode: &str) -> Option<Theme> {
    match mode {
        "light" => Some(Theme::Light),
        "dark" => Some(Theme::Dark),
        _ => None,
    }
}

/// 프로그램이 끝나기 직전에 부를 일(돌고 있는 외부 프로그램 정리 등)
static ON_EXIT: std::sync::OnceLock<Box<dyn Fn() + Send + Sync>> = std::sync::OnceLock::new();

pub fn on_exit(work: impl Fn() + Send + Sync + 'static) {
    let _ = ON_EXIT.set(Box::new(work));
}

/// 단축키 칸. 기기 칸(Board)과 따로 그린다.
static SHORTCUTS: std::sync::OnceLock<&'static crate::ShortcutsPanel> = std::sync::OnceLock::new();

pub fn with_shortcuts(panel: &'static crate::ShortcutsPanel) {
    let _ = SHORTCUTS.set(panel);
}

/// 창과 화면. 필드 순서대로 놓이므로 화면이 자료 폴더 설정과 창보다 먼저 사라진다.
struct Parts {
    webview: WebView,
    /// 화면이 사는 동안 살아 있어야 한다.
    _context: WebContext,
    window: Window,
}

/// 설정 창을 띄운다. 창을 닫으면 프로세스를 끝낸다(연결 스레드를 기다리지 않는다).
/// `mode_file` 에 고른 화면 모드를 저장한다. `data_folder` 는 윈도우 WebView2 가 자료를 두는 곳이다.
pub fn open(board: &'static Board, mode_file: PathBuf, data_folder: PathBuf) -> Result<(), String> {
    let saved = std::fs::read_to_string(&mode_file)
        .ok()
        .and_then(|text| mode(&text))
        .unwrap_or("auto");
    let event_loop = EventLoopBuilder::<Change>::with_user_event().build();
    let parts = build(&event_loop, board, saved, &data_folder)?;
    event_loop.run(move |event, _, flow| {
        *flow = ControlFlow::Wait;
        // Dock 의 종료, 로그아웃처럼 창 닫기를 거치지 않고 끝날 때
        if matches!(event, Event::LoopDestroyed) {
            if let Some(work) = ON_EXIT.get() {
                work();
            }
            return;
        }
        if handle(&parts, event, &mode_file) {
            if let Some(work) = ON_EXIT.get() {
                work();
            }
            std::process::exit(0);
        }
    })
}

fn build(
    event_loop: &EventLoop<Change>,
    board: &'static Board,
    saved: &'static str,
    data_folder: &Path,
) -> Result<Parts, String> {
    let proxy = event_loop.create_proxy();
    if let Some(panel) = SHORTCUTS.get() {
        let proxy = event_loop.create_proxy();
        panel.on_change(move |snapshot| {
            let _ = proxy.send_event(Change::Script(format!("renderShortcuts({snapshot})")));
        });
    }
    board.on_change(move |snapshot| {
        let _ = proxy.send_event(Change::Render(snapshot));
    });
    let ipc = event_loop.create_proxy();
    let builder = WindowBuilder::new()
        .with_title("SideDesk 설정")
        .with_inner_size(LogicalSize::new(1120.0, 760.0))
        .with_min_inner_size(LogicalSize::new(820.0, 560.0))
        .with_theme(theme(saved));
    // 맥은 제목 줄을 감추고 화면 위쪽 막대가 그 자리를 대신한다.
    #[cfg(target_os = "macos")]
    let builder = builder
        .with_titlebar_transparent(true)
        .with_title_hidden(true)
        .with_fullsize_content_view(true);
    let window = builder
        .build(event_loop)
        .map_err(|error| format!("창을 열지 못함: {error}"))?;
    // 윈도우 WebView2 는 자료 폴더를 기본으로 실행 파일 옆에 만든다. 쓰기 금지 폴더에서도 뜨게 설정 폴더에 둔다.
    // 맥은 이 값을 쓰지 않는다.
    let mut context = WebContext::new(Some(data_folder.to_path_buf()));
    let builder = WebViewBuilder::new_with_web_context(&mut context)
        .with_custom_protocol(SCHEME.to_owned(), |_id, request| {
            let (kind, body) =
                asset(request.uri().path()).unwrap_or(("text/plain; charset=utf-8", b"not found"));
            Response::builder()
                .header(CONTENT_TYPE, kind)
                .body(Cow::Borrowed(body))
                .expect("응답")
        })
        .with_initialization_script(format!(
            "document.documentElement.dataset.mode = '{saved}';\
             document.documentElement.dataset.theme = '{saved}' === 'auto' \
             ? (matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light') : '{saved}';{}",
            if cfg!(target_os = "macos") {
                "document.documentElement.classList.add('mac');"
            } else {
                ""
            }
        ))
        .with_ipc_handler(move |request| {
            let body = request.body();
            if body == "drag" {
                let _ = ipc.send_event(Change::Drag);
            } else if let Some(chosen) = body.strip_prefix("mode:").and_then(mode) {
                let _ = ipc.send_event(Change::Mode(chosen));
            } else if !SHORTCUTS.get().is_some_and(|panel| panel.command(body)) {
                board.command(body);
            }
        })
        .with_url(START);
    // 윈도우 WebView2 는 처음 테마를 따로 정해야 첫 화면이 고른 모드로 뜬다.
    #[cfg(target_os = "windows")]
    let builder = {
        use wry::WebViewBuilderExtWindows;
        builder.with_theme(match saved {
            "light" => wry::Theme::Light,
            "dark" => wry::Theme::Dark,
            _ => wry::Theme::Auto,
        })
    };
    let webview = builder
        .build(&window)
        .map_err(|error| format!("화면을 열지 못함: {error}"))?;

    #[cfg(target_os = "macos")]
    unified_title(&window);

    Ok(Parts {
        webview,
        _context: context,
        window,
    })
}

/// 사건 하나를 처리한다. 창을 닫으라는 사건이면 true.
fn handle(parts: &Parts, event: Event<'_, Change>, mode_file: &Path) -> bool {
    let Parts {
        webview, window, ..
    } = parts;
    match event {
        Event::UserEvent(Change::Render(snapshot)) => {
            let _ = webview.evaluate_script(&format!("render({snapshot})"));
        }
        Event::UserEvent(Change::Script(script)) => {
            let _ = webview.evaluate_script(&script);
        }
        Event::UserEvent(Change::Drag) => {
            let _ = window.drag_window();
        }
        Event::UserEvent(Change::Mode(chosen)) => {
            window.set_theme(theme(chosen));
            #[cfg(target_os = "windows")]
            {
                use wry::WebViewExtWindows;
                let _ = webview.set_theme(match chosen {
                    "light" => wry::Theme::Light,
                    "dark" => wry::Theme::Dark,
                    _ => wry::Theme::Auto,
                });
            }
            let _ = webview.evaluate_script(&format!(
                "document.documentElement.dataset.mode = '{chosen}'; showMode();"
            ));
            let _ = std::fs::write(mode_file, chosen);
        }
        Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            ..
        } => return true,
        _ => {}
    }
    false
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use sidedesk_core::{Identity, KEY_LEN};
    use sidedesk_domain::{Keys, PairRequests, Paired, PortError};
    use tao::event::StartCause;
    use tao::platform::run_return::EventLoopExtRunReturn;
    use tao::platform::windows::EventLoopBuilderExtWindows;

    use super::*;
    use crate::board::Computer;

    struct NoKeys;

    impl Keys for NoKeys {
        fn identity(&self) -> Result<Identity, PortError> {
            Ok(Identity::generate())
        }
        fn trusted(&self) -> Result<Vec<Paired>, PortError> {
            Ok(Vec::new())
        }
        fn trust(&self, _: [u8; KEY_LEN], _: &str, _: &str) -> Result<(), PortError> {
            Ok(())
        }
        fn update(&self, _: [u8; KEY_LEN], _: &str, _: &str) -> Result<(), PortError> {
            Ok(())
        }
        fn forget(&self, _: [u8; KEY_LEN]) -> Result<(), PortError> {
            Ok(())
        }
    }

    /// 창을 띄우고, 화면이 그려진 뒤 컴퓨터 이름 칸의 글자를 읽어 맞춘다.
    #[test]
    #[ignore = "화면 세션에 창을 띄운다. CI 윈도우 작업에서 돌린다"]
    fn windows_only_the_window_opens_and_shows_the_computer_name() {
        let name = "시험-PC";
        let board: &'static Board = Box::leak(Box::new(Board::new(
            Computer {
                name: name.to_owned(),
                version: "0.0.0".to_owned(),
                key: [7; KEY_LEN],
            },
            Box::leak(Box::new(NoKeys)),
            Box::leak(Box::<PairRequests>::default()),
        )));
        let folder = std::env::temp_dir().join(format!("sidedesk-view-{}", std::process::id()));
        let mode_file = folder.join("mode.txt");
        let mut event_loop = EventLoopBuilder::<Change>::with_user_event()
            .with_any_thread(true)
            .build();
        let parts = build(&event_loop, board, "auto", &folder.join("WebView2")).unwrap();
        let reader = event_loop.create_proxy();
        let read: Mutex<Option<String>> = Mutex::new(None);
        let started = Instant::now();
        let limit = started + Duration::from_secs(60);
        event_loop.run_return(|event, _, flow| {
            *flow = ControlFlow::WaitUntil(limit);
            match event {
                Event::NewEvents(StartCause::ResumeTimeReached { .. }) => {
                    *flow = ControlFlow::Exit;
                }
                Event::UserEvent(Change::Read(text)) => {
                    *read.lock().unwrap() = Some(text);
                    *flow = ControlFlow::Exit;
                }
                Event::UserEvent(Change::Render(_)) => {
                    handle(&parts, event, &mode_file);
                    let reader = reader.clone();
                    let _ = parts.webview.evaluate_script_with_callback(
                        "document.getElementById('computer-name').textContent",
                        move |text| {
                            let _ = reader.send_event(Change::Read(text));
                        },
                    );
                }
                other => {
                    handle(&parts, other, &mode_file);
                }
            }
        });
        eprintln!("창이 뜨고 글자를 읽기까지 {:?}", started.elapsed());
        drop(parts);
        let _ = std::fs::remove_dir_all(&folder);
        let read = read
            .into_inner()
            .unwrap()
            .expect("60초 안에 화면을 읽지 못함");
        assert_eq!(read, format!("\"{name}\""));
    }
}
