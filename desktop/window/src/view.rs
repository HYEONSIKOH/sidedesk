use std::borrow::Cow;
use std::path::{Path, PathBuf};

use tao::dpi::LogicalSize;
use tao::event::{Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy, EventLoopWindowTarget};
#[cfg(target_os = "macos")]
use tao::platform::macos::{WindowBuilderExtMacOS, WindowExtMacOS};
use tao::window::{Theme, Window, WindowBuilder};
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};
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
    /// 화면이 다 떠서 지금 값을 처음 그릴 차례
    Ready,
    /// 메뉴 막대(트레이) 메뉴
    Menu(MenuId),
    /// 단축키 칸의 고르기 창(폴더면 true). 웹뷰 메시지 처리기 안에서 모달 창을 돌리면
    /// 다시 들어오기 문제가 있어 이벤트 루프에서 띄운다.
    Pick(bool),
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

/// 일반 칸의 음악 안내
static MUSIC: std::sync::OnceLock<&'static crate::MusicPanel> = std::sync::OnceLock::new();

pub fn with_music(panel: &'static crate::MusicPanel) {
    let _ = MUSIC.set(panel);
}

/// 창과 화면. 필드 순서대로 놓이므로 화면이 자료 폴더 설정과 창보다 먼저 사라진다.
struct Parts {
    webview: WebView,
    /// 화면이 사는 동안 살아 있어야 한다.
    _context: WebContext,
    window: Window,
    board: &'static Board,
}

/// 창 닫기: 띄운 것(화면, 자료 폴더 설정, 창)을 버린다. 다시 열 길(메뉴 막대 아이콘)이 없으면 끝내야 하니 true.
fn close<T>(shown: &mut Option<T>, can_reopen: bool) -> bool {
    drop(shown.take());
    !can_reopen
}

const TRAY_SIDE: u32 = 32;

/// 아이콘 모양(폰 테두리와 아래 점)에 드는 칸인지
fn tray_shape(x: i64, y: i64) -> bool {
    let inside = (9..=22).contains(&x) && (3..=28).contains(&y);
    let edge = inside && (x <= 11 || x >= 20 || y <= 5 || y >= 26);
    let corner = (x == 9 || x == 22) && (y == 3 || y == 28);
    let dot = (15..=16).contains(&x) && (23..=24).contains(&y);
    (edge && !corner) || dot
}

/// 메뉴 막대 아이콘: 둥근 네모(폰) 테두리. 맥은 검정 템플릿(밝기에 맞춰 색이 바뀜).
/// 윈도우는 작업 표시줄이 밝든 어둡든 보이게 흰 모양에 검은 테두리를 두른다.
fn tray_pixels(outlined: bool) -> Vec<u8> {
    let side = i64::from(TRAY_SIDE);
    let mut rgba = vec![0u8; (TRAY_SIDE * TRAY_SIDE * 4) as usize];
    for y in 0..side {
        for x in 0..side {
            let at = ((y * side + x) * 4) as usize;
            if tray_shape(x, y) {
                let fill = if outlined { 255 } else { 0 };
                rgba[at..at + 4].copy_from_slice(&[fill, fill, fill, 255]);
            } else if outlined && (-1..=1).any(|dy| (-1..=1).any(|dx| tray_shape(x + dx, y + dy))) {
                rgba[at..at + 4].copy_from_slice(&[0, 0, 0, 255]);
            }
        }
    }
    rgba
}

fn tray_icon() -> Option<Icon> {
    let rgba = tray_pixels(cfg!(not(target_os = "macos")));
    Icon::from_rgba(rgba, TRAY_SIDE, TRAY_SIDE).ok()
}

fn make_tray(open_item: &MenuItem, quit_item: &MenuItem) -> Option<TrayIcon> {
    let menu = Menu::new();
    let _ = menu.append(open_item);
    let _ = menu.append(quit_item);
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
    builder.build().ok()
}

/// 설정 창을 띄우고 메뉴 막대(윈도우는 시스템 트레이)에 상주한다. 창을 닫으면 창과 화면만 버리고
/// 메뉴의 "설정 열기"로 같은 자료 폴더로 새로 만든다. "끝내기"로 프로세스를 끝낸다(연결 스레드를 기다리지 않는다).
/// `mode_file` 에 고른 화면 모드를 저장한다. `data_folder` 는 윈도우 WebView2 가 자료를 두는 곳이다.
pub fn open(board: &'static Board, mode_file: PathBuf, data_folder: PathBuf) -> Result<(), String> {
    let mut saved = std::fs::read_to_string(&mode_file)
        .ok()
        .and_then(|text| mode(&text))
        .unwrap_or("auto");
    let event_loop = EventLoopBuilder::<Change>::with_user_event().build();
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
    let proxy = event_loop.create_proxy();
    let open_item = MenuItem::new("설정 열기", true, None);
    let quit_item = MenuItem::new("끝내기", true, None);
    let mut shown = Some(build(
        &event_loop,
        proxy.clone(),
        board,
        saved,
        &data_folder,
    )?);
    let mut tray: Option<TrayIcon> = None;
    let quit = || {
        if let Some(work) = ON_EXIT.get() {
            work();
        }
        std::process::exit(0)
    };
    event_loop.run(move |event, target, flow| {
        *flow = ControlFlow::Wait;
        match &event {
            // Dock 의 종료, 로그아웃처럼 창 닫기를 거치지 않고 끝날 때
            Event::LoopDestroyed => {
                // 윈도우는 알림 영역 아이콘을 지우지 않으면 빈 자리가 남는다.
                drop(tray.take());
                if let Some(work) = ON_EXIT.get() {
                    work();
                }
                return;
            }
            // 맥은 이벤트 루프가 돈 뒤에 메뉴 막대 아이콘을 만들어야 한다.
            Event::NewEvents(StartCause::Init) => {
                tray = make_tray(&open_item, &quit_item);
                return;
            }
            Event::UserEvent(Change::Menu(id)) => {
                if id == quit_item.id() {
                    drop(tray.take());
                    quit();
                } else if id == open_item.id() {
                    match &shown {
                        Some(parts) => parts.window.set_focus(),
                        None => {
                            shown = build(target, proxy.clone(), board, saved, &data_folder).ok();
                        }
                    }
                }
                return;
            }
            // 맥 Dock 아이콘을 눌렀을 때
            Event::Reopen { .. } => {
                if shown.is_none() {
                    shown = build(target, proxy.clone(), board, saved, &data_folder).ok();
                }
                return;
            }
            Event::UserEvent(Change::Mode(chosen)) => saved = chosen,
            _ => {}
        }
        let Some(parts) = &shown else {
            return;
        };
        if handle(parts, event, &mode_file) && close(&mut shown, tray.is_some()) {
            quit();
        }
    })
}

/// 창과 화면을 만든다. 다시 열 때도 같은 `data_folder` 로 만든다.
fn build(
    target: &EventLoopWindowTarget<Change>,
    ipc: EventLoopProxy<Change>,
    board: &'static Board,
    saved: &'static str,
    data_folder: &Path,
) -> Result<Parts, String> {
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
            } else if body.trim() == r#"{"ready":true}"# {
                let _ = ipc.send_event(Change::Ready);
            } else if let Some(folder) = crate::shortcuts::pick_request(body) {
                let _ = ipc.send_event(Change::Pick(folder));
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

    Ok(Parts {
        webview,
        _context: context,
        window,
        board,
    })
}

/// 사건 하나를 처리한다. 창을 닫으라는 사건이면 true.
fn handle(parts: &Parts, event: Event<'_, Change>, mode_file: &Path) -> bool {
    let Parts {
        webview,
        window,
        board,
        ..
    } = parts;
    match event {
        // 창을 새로 열 때마다 지금 값을 처음 그린다.
        Event::UserEvent(Change::Ready) => {
            let _ = webview.evaluate_script(&format!("render({})", board.snapshot()));
            if let Some(panel) = SHORTCUTS.get() {
                let _ = webview.evaluate_script(&format!("renderShortcuts({})", panel.snapshot()));
            }
            if let Some(panel) = MUSIC.get() {
                let _ = webview.evaluate_script(&format!("renderMusic({})", panel.snapshot()));
            }
        }
        Event::UserEvent(Change::Render(snapshot)) => {
            let _ = webview.evaluate_script(&format!("render({snapshot})"));
        }
        Event::UserEvent(Change::Script(script)) => {
            let _ = webview.evaluate_script(&script);
        }
        Event::UserEvent(Change::Pick(folder)) => {
            if let Some(panel) = SHORTCUTS.get() {
                panel.pick(folder);
            }
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

    /// 트레이 아이콘을 만들고, 창을 띄워 컴퓨터 이름을 읽고, 창을 닫았다가(화면, 자료 설정, 창을 버림)
    /// 같은 자료 폴더로 다시 열어 또 읽는다. 끝나면 트레이 아이콘을 지운다.
    #[test]
    #[ignore = "화면 세션에 창과 트레이 아이콘을 띄운다. CI 윈도우 작업에서 돌린다"]
    fn windows_only_the_window_opens_closes_and_reopens_from_the_tray() {
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
        let data = folder.join("WebView2");
        let mode_file = folder.join("mode.txt");
        let mut event_loop = EventLoopBuilder::<Change>::with_user_event()
            .with_any_thread(true)
            .build();
        let proxy = event_loop.create_proxy();
        let open_item = MenuItem::new("설정 열기", true, None);
        let quit_item = MenuItem::new("끝내기", true, None);
        let mut shown = Some(build(&event_loop, proxy.clone(), board, "auto", &data).unwrap());
        let mut tray: Option<TrayIcon> = None;
        let mut tray_made = false;
        let reads: Mutex<Vec<String>> = Mutex::new(Vec::new());
        let started = Instant::now();
        let limit = started + Duration::from_secs(90);
        event_loop.run_return(|event, target, flow| {
            *flow = ControlFlow::WaitUntil(limit);
            match event {
                Event::NewEvents(StartCause::Init) => {
                    tray = make_tray(&open_item, &quit_item);
                    tray_made = tray.is_some();
                }
                Event::NewEvents(StartCause::ResumeTimeReached { .. }) => {
                    *flow = ControlFlow::Exit;
                }
                Event::UserEvent(Change::Read(text)) => {
                    let count = {
                        let mut reads = reads.lock().unwrap();
                        reads.push(text);
                        reads.len()
                    };
                    // 처음 읽은 뒤에는 창을 닫았다가 같은 자료 폴더로 다시 연다(트레이의 "설정 열기"와 같은 길).
                    assert!(close(&mut shown, tray.is_some()) == !tray_made);
                    if count == 1 {
                        shown = build(target, proxy.clone(), board, "auto", &data).ok();
                        assert!(shown.is_some(), "다시 열지 못함");
                    } else {
                        drop(tray.take());
                        *flow = ControlFlow::Exit;
                    }
                }
                Event::UserEvent(Change::Ready) => {
                    let Some(parts) = &shown else {
                        return;
                    };
                    handle(parts, event, &mode_file);
                    let reader = proxy.clone();
                    let _ = parts.webview.evaluate_script_with_callback(
                        "document.getElementById('computer-name').textContent",
                        move |text| {
                            let _ = reader.send_event(Change::Read(text));
                        },
                    );
                }
                other => {
                    if let Some(parts) = &shown {
                        handle(parts, other, &mode_file);
                    }
                }
            }
        });
        eprintln!(
            "트레이 {tray_made}, 두 번 열고 읽기까지 {:?}",
            started.elapsed()
        );
        drop(shown);
        drop(tray);
        let _ = std::fs::remove_dir_all(&folder);
        assert!(tray_made, "트레이 아이콘을 만들지 못함");
        let reads = reads.into_inner().unwrap();
        assert_eq!(
            reads,
            vec![format!("\"{name}\""), format!("\"{name}\"")],
            "두 번 다 화면을 읽어야 함"
        );
    }

    #[test]
    fn the_tray_icon_is_visible_on_light_and_dark_taskbars() {
        let pixels = tray_pixels(true);
        let colors: std::collections::HashSet<[u8; 4]> =
            pixels.chunks(4).map(|p| [p[0], p[1], p[2], p[3]]).collect();
        assert!(colors.contains(&[255, 255, 255, 255]), "흰 모양");
        assert!(colors.contains(&[0, 0, 0, 255]), "검은 테두리");
        assert!(tray_icon().is_some());
    }

    #[test]
    fn pick_requests_go_to_the_event_loop() {
        assert_eq!(
            crate::shortcuts::pick_request(r#"{"shortcut-pick":true}"#),
            Some(false)
        );
        assert_eq!(
            crate::shortcuts::pick_request(r#"{"shortcut-pick-folder":true}"#),
            Some(true)
        );
        assert_eq!(
            crate::shortcuts::pick_request(r#"{"shortcut-save":{}}"#),
            None
        );
    }
}

#[cfg(test)]
mod close_tests {
    use super::close;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Probe(Arc<AtomicUsize>);

    impl Drop for Probe {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn closing_drops_what_was_shown_and_quits_only_without_a_tray() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let mut shown = Some(Probe(Arc::clone(&dropped)));
        assert!(!close(&mut shown, true));
        assert!(shown.is_none());
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
        // 이미 닫힌 뒤 다시 닫아도 아무것도 버리지 않는다.
        assert!(!close(&mut shown, true));
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
        let mut shown = Some(Probe(Arc::clone(&dropped)));
        assert!(close(&mut shown, false));
        assert_eq!(dropped.load(Ordering::SeqCst), 2);
    }
}
