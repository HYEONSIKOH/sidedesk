use std::borrow::Cow;
use std::path::PathBuf;

use tao::dpi::LogicalSize;
use tao::event::{Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy, EventLoopWindowTarget};
#[cfg(target_os = "macos")]
use tao::platform::macos::{WindowBuilderExtMacOS, WindowExtMacOS};
use tao::window::{Theme, Window, WindowBuilder};
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};
use wry::http::{Response, header::CONTENT_TYPE};
use wry::{WebView, WebViewBuilder};

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
    /// 화면이 다 떠서 지금 값을 처음 그릴 차례
    Ready,
    /// 메뉴 막대(트레이) 메뉴
    Menu(MenuId),
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

/// 일반 칸의 음악 안내
static MUSIC: std::sync::OnceLock<&'static crate::MusicPanel> = std::sync::OnceLock::new();

pub fn with_music(panel: &'static crate::MusicPanel) {
    let _ = MUSIC.set(panel);
}

/// 띄운 설정 창. 웹뷰를 창보다 먼저 버린다.
struct Shown {
    webview: WebView,
    window: Window,
}

/// 메뉴 막대 아이콘: 둥근 네모(폰) 테두리
fn tray_icon() -> Option<Icon> {
    const SIDE: u32 = 32;
    let mut rgba = vec![0u8; (SIDE * SIDE * 4) as usize];
    for y in 0..SIDE {
        for x in 0..SIDE {
            let inside = (9..=22).contains(&x) && (3..=28).contains(&y);
            let edge = inside && (x <= 11 || x >= 20 || y <= 5 || y >= 26);
            let corner = (x == 9 || x == 22) && (y == 3 || y == 28);
            let dot = (15..=16).contains(&x) && (23..=24).contains(&y);
            if (edge && !corner) || dot {
                let at = ((y * SIDE + x) * 4) as usize;
                rgba[at..at + 4].copy_from_slice(&[0, 0, 0, 255]);
            }
        }
    }
    Icon::from_rgba(rgba, SIDE, SIDE).ok()
}

/// 지금 값을 화면에 처음 그린다(창을 새로 열 때마다).
fn paint_all(proxy: &EventLoopProxy<Change>, board: &'static Board) {
    let _ = proxy.send_event(Change::Render(board.snapshot()));
    if let Some(panel) = SHORTCUTS.get() {
        let _ = proxy.send_event(Change::Script(format!(
            "renderShortcuts({})",
            panel.snapshot()
        )));
    }
    if let Some(panel) = MUSIC.get() {
        let _ = proxy.send_event(Change::Script(format!("renderMusic({})", panel.snapshot())));
    }
}

fn show(
    target: &EventLoopWindowTarget<Change>,
    proxy: EventLoopProxy<Change>,
    board: &'static Board,
    saved: &'static str,
) -> Result<Shown, String> {
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
        .build(target)
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
                let _ = proxy.send_event(Change::Drag);
            } else if let Some(chosen) = body.strip_prefix("mode:").and_then(mode) {
                let _ = proxy.send_event(Change::Mode(chosen));
            } else if body.trim() == r#"{"ready":true}"# {
                let _ = proxy.send_event(Change::Ready);
            } else if !SHORTCUTS.get().is_some_and(|panel| panel.command(body))
                && !MUSIC.get().is_some_and(|panel| panel.command(body))
            {
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
    Ok(Shown { webview, window })
}

/// 설정 창을 띄우고 메뉴 막대(트레이)에 상주한다. 창을 닫으면 창과 웹뷰만 버리고
/// 메뉴의 "설정 열기"로 새로 만든다. "끝내기"로 프로세스를 끝낸다(연결 스레드를 기다리지 않는다).
/// `mode_file` 에 고른 화면 모드를 저장한다.
pub fn open(board: &'static Board, mode_file: PathBuf) -> Result<(), String> {
    let mut saved = std::fs::read_to_string(&mode_file)
        .ok()
        .and_then(|text| mode(&text))
        .unwrap_or("auto");
    let event_loop = EventLoopBuilder::<Change>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    if let Some(panel) = SHORTCUTS.get() {
        let proxy = event_loop.create_proxy();
        panel.on_change(move |snapshot| {
            let _ = proxy.send_event(Change::Script(format!("renderShortcuts({snapshot})")));
        });
    }
    if let Some(panel) = MUSIC.get() {
        let proxy = event_loop.create_proxy();
        panel.on_change(move |snapshot| {
            let _ = proxy.send_event(Change::Script(format!("renderMusic({snapshot})")));
        });
    }
    {
        let proxy = event_loop.create_proxy();
        board.on_change(move |snapshot| {
            let _ = proxy.send_event(Change::Render(snapshot));
        });
    }
    {
        let proxy = std::sync::Mutex::new(event_loop.create_proxy());
        MenuEvent::set_event_handler(Some(std::sync::Arc::new(move |event: MenuEvent| {
            if let Ok(proxy) = proxy.lock() {
                let _ = proxy.send_event(Change::Menu(event.id));
            }
        })));
    }
    let open_item = MenuItem::new("설정 열기", true, None);
    let quit_item = MenuItem::new("끝내기", true, None);
    let mut shown = Some(show(&event_loop, proxy.clone(), board, saved)?);
    let mut tray: Option<TrayIcon> = None;
    let quit = || {
        if let Some(work) = ON_EXIT.get() {
            work();
        }
        std::process::exit(0)
    };

    event_loop.run(move |event, target, flow| {
        *flow = ControlFlow::Wait;
        match event {
            // 맥은 이벤트 루프가 돈 뒤에 메뉴 막대 아이콘을 만들어야 한다.
            Event::NewEvents(StartCause::Init) => {
                let menu = Menu::new();
                let _ = menu.append(&open_item);
                let _ = menu.append(&quit_item);
                let mut builder = TrayIconBuilder::new()
                    .with_tooltip("SideDesk")
                    .with_menu(Box::new(menu));
                if let Some(icon) = tray_icon() {
                    // 맥은 템플릿이라 메뉴 막대 밝기에 맞춰 색이 바뀐다.
                    #[cfg(target_os = "macos")]
                    {
                        builder = builder.with_icon_templated(icon);
                    }
                    #[cfg(not(target_os = "macos"))]
                    {
                        builder = builder.with_icon(icon);
                    }
                }
                tray = builder.build().ok();
            }
            Event::UserEvent(Change::Menu(id)) => {
                if id == *quit_item.id() {
                    drop(tray.take());
                    quit();
                } else if id == *open_item.id() {
                    match &shown {
                        Some(now) => now.window.set_focus(),
                        None => shown = show(target, proxy.clone(), board, saved).ok(),
                    }
                }
            }
            // 맥 Dock 아이콘을 눌렀을 때
            Event::Reopen { .. } if shown.is_none() => {
                shown = show(target, proxy.clone(), board, saved).ok();
            }
            Event::UserEvent(Change::Ready) => paint_all(&proxy, board),
            Event::UserEvent(Change::Render(snapshot)) => {
                if let Some(now) = &shown {
                    let _ = now.webview.evaluate_script(&format!("render({snapshot})"));
                }
            }
            Event::UserEvent(Change::Script(script)) => {
                if let Some(now) = &shown {
                    let _ = now.webview.evaluate_script(&script);
                }
            }
            Event::UserEvent(Change::Drag) => {
                if let Some(now) = &shown {
                    let _ = now.window.drag_window();
                }
            }
            Event::UserEvent(Change::Mode(chosen)) => {
                saved = chosen;
                let _ = std::fs::write(&mode_file, chosen);
                let Some(now) = &shown else {
                    return;
                };
                now.window.set_theme(theme(chosen));
                #[cfg(target_os = "windows")]
                {
                    use wry::WebViewExtWindows;
                    let _ = now.webview.set_theme(match chosen {
                        "light" => wry::Theme::Light,
                        "dark" => wry::Theme::Dark,
                        _ => wry::Theme::Auto,
                    });
                }
                let _ = now.webview.evaluate_script(&format!(
                    "document.documentElement.dataset.mode = '{chosen}'; showMode();"
                ));
            }
            // 창을 닫으면 웹뷰와 창만 버린다(웹뷰 도우미 프로세스도 끝난다).
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                if let Some(Shown { webview, window }) = shown.take() {
                    drop(webview);
                    drop(window);
                }
                // 메뉴 막대 아이콘을 못 만들었으면 다시 열 길이 없으니 끝낸다.
                if tray.is_none() {
                    quit();
                }
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
