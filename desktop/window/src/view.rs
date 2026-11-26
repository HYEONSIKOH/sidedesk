use std::borrow::Cow;
use std::path::PathBuf;

use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
#[cfg(target_os = "macos")]
use tao::platform::macos::{WindowBuilderExtMacOS, WindowExtMacOS};
use tao::window::{Theme, WindowBuilder};
use wry::WebViewBuilder;
use wry::http::{Response, header::CONTENT_TYPE};

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
    Drag,
    Mode(&'static str),
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

/// 설정 창을 띄운다. 창을 닫으면 프로세스를 끝낸다(연결 스레드를 기다리지 않는다).
/// `mode_file` 에 고른 화면 모드를 저장한다.
pub fn open(board: &'static Board, mode_file: PathBuf) -> Result<(), String> {
    let saved = std::fs::read_to_string(&mode_file)
        .ok()
        .and_then(|text| mode(&text))
        .unwrap_or("auto");
    let event_loop = EventLoopBuilder::<Change>::with_user_event().build();
    let proxy = event_loop.create_proxy();
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
        .build(&event_loop)
        .map_err(|error| format!("창을 열지 못함: {error}"))?;
    let builder = WebViewBuilder::new()
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
            } else {
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

    event_loop.run(move |event, _, flow| {
        *flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(Change::Render(snapshot)) => {
                let _ = webview.evaluate_script(&format!("render({snapshot})"));
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
                let _ = std::fs::write(&mode_file, chosen);
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                if let Some(work) = ON_EXIT.get() {
                    work();
                }
                std::process::exit(0)
            }
            // Dock 의 종료, 로그아웃처럼 창 닫기를 거치지 않고 끝날 때
            Event::LoopDestroyed => {
                if let Some(work) = ON_EXIT.get() {
                    work();
                }
            }
            _ => {}
        }
    })
}
