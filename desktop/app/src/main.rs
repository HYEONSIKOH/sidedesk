mod prefs;
mod shortcuts_file;
mod store;

use std::io::BufRead;
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self, Receiver};
use std::thread;

use sidedesk_domain::{
    Active, Cadence, ComputerName, Context, Ended, Hub, Keys, Note, PairRequests, PhoneLinks,
    Prompt, Report, Timing, keep_connected, serve,
};
use sidedesk_link::AdbLinks;
#[cfg(target_os = "macos")]
use sidedesk_mac::{
    MacGet, MacKeychain, MacLauncher, MacMeter, MacName as Name, MacPlayer, MacProcesses,
    MacSpeaker,
};
use sidedesk_window::{Board, Computer};
#[cfg(target_os = "windows")]
use sidedesk_windows::{
    WindowsLauncher, WindowsMeter, WindowsName as Name, WindowsPlayer, WindowsProcesses,
    WindowsSecrets, WindowsSpeaker,
};

use crate::prefs::WeatherPrefs;
use crate::shortcuts_file::ShortcutsFile;
use crate::store::FileKeys;

const USAGE: &str = "사용: sidedesk (설정 창) | sidedesk pair (터미널에서 기기 등록) | sidedesk run (창 없이 연결 유지)";

fn main() -> ExitCode {
    let command = std::env::args().nth(1);
    let result = match command.as_deref() {
        Some("pair") => pair(),
        Some("run") => run(),
        None => window(),
        _ => Err(USAGE.to_owned()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

struct Terminal;

impl Report for Terminal {
    fn note(&self, phone: &str, note: Note) {
        match note {
            Note::Connected { .. } => println!("{phone}: 연결됨"),
            Note::Paired { .. } => println!("{phone}: 등록함"),
            Note::Lost(reason) => eprintln!("{phone}: {reason}"),
            Note::Refused(reason) => eprintln!("{phone}: {reason}. 기기를 다시 꽂을 때까지 기다림"),
        }
    }
}

impl Prompt for Terminal {
    fn ask(&self, _phone: &str, code: &str) -> Receiver<bool> {
        println!("기기에 표시된 번호가 {code} 이면 y, 아니면 n 을 입력하고 엔터");
        let (answer, receiver) = mpsc::channel();
        thread::spawn(move || {
            let mut line = String::new();
            if std::io::stdin().lock().read_line(&mut line).is_ok() {
                let _ = answer.send(line.trim().eq_ignore_ascii_case("y"));
            }
        });
        receiver
    }
}

/// 성능과 프로세스만 모으는 묶음. 날씨는 `hub` 가 더한다.
#[cfg(target_os = "macos")]
fn collectors() -> Hub {
    Hub::new(
        Box::new(MacMeter::default()),
        Box::new(MacProcesses::default()),
        Cadence::default(),
    )
}

#[cfg(target_os = "windows")]
fn collectors() -> Hub {
    Hub::new(
        Box::new(WindowsMeter),
        Box::new(WindowsProcesses),
        Cadence::default(),
    )
}

/// 단축키 설정 창 도우미
type Help = Arc<dyn sidedesk_domain::ShortcutHelp>;

/// 맥은 날씨를 받고, 윈도우는 받지 않는다(설정 창에 "받지 않음").
#[cfg(target_os = "macos")]
fn hub(folder: &Path) -> (Hub, Arc<WeatherPrefs>, Help) {
    let get: Arc<dyn sidedesk_weather::Get> = Arc::new(MacGet::default());
    let prefs = Arc::new(WeatherPrefs::new(
        folder.join("weather.txt"),
        Box::new(MacKeychain::default()),
        Some(Arc::clone(&get)),
    ));
    let weather = sidedesk_weather::Chooser::new(prefs.clone(), get);
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let hub = collectors().with_weather(
        Box::new(weather),
        sidedesk_domain::WeatherCadence::default(),
    );
    // AI 세션 기록과 gh, glab 은 홈 폴더 아래를 본다. 윈도우는 아직 없음(컴파일만).
    let hub = match home {
        Some(home) => hub.with_ai(
            Box::new(sidedesk_agents::Agents::new(home.clone())),
            Some(Box::new(sidedesk_forge::Forgery::system(home))),
            sidedesk_domain::AiCadence::default(),
        ),
        None => hub,
    };
    let launcher = Arc::new(MacLauncher::new(
        std::env::var_os("HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_default(),
    ));
    let deck = sidedesk_domain::Deck::new(Box::new(ShortcutsFile::new(folder)), launcher.clone());
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_default();
    let cache = home.join("Library/Caches/SideDesk");
    let jukebox = sidedesk_domain::Jukebox::new(
        Box::new(MacPlayer::new(home, cache)),
        Box::new(MacSpeaker),
        sidedesk_domain::MusicCadence::default(),
    );
    (
        hub.with_shortcuts(deck).with_music(Arc::new(jukebox)),
        prefs,
        launcher,
    )
}

#[cfg(target_os = "windows")]
fn hub(folder: &Path) -> (Hub, Arc<WeatherPrefs>, Help) {
    let prefs = Arc::new(WeatherPrefs::new(
        folder.join("weather.txt"),
        Box::new(WindowsSecrets),
        None,
    ));
    let launcher = Arc::new(WindowsLauncher);
    let deck = sidedesk_domain::Deck::new(Box::new(ShortcutsFile::new(folder)), launcher.clone());
    let jukebox = sidedesk_domain::Jukebox::new(
        Box::new(WindowsPlayer),
        Box::new(WindowsSpeaker),
        sidedesk_domain::MusicCadence::default(),
    );
    (
        collectors()
            .with_shortcuts(deck)
            .with_music(Arc::new(jukebox)),
        prefs,
        launcher,
    )
}

fn run() -> Result<(), String> {
    let folder = store::folder().map_err(|e| e.0)?;
    let _lock = store::lock(&folder).map_err(|e| e.0)?;
    let (hub, _, _) = hub(&folder);
    let keys = FileKeys::new(folder);
    let context = Context {
        name: &Name,
        keys: &keys,
        prompt: &Terminal,
        report: &Terminal,
        requests: &PairRequests::default(),
        active: &Active::default(),
        hub: &hub,
        timing: Timing::default(),
    };
    println!("기기를 기다리는 중. 끝내려면 Ctrl+C");
    thread::scope(|scope| {
        scope.spawn(|| hub.run());
        scope.spawn(|| hub.run_weather());
        scope.spawn(|| hub.run_ai());
        scope.spawn(|| hub.run_forge());
        scope.spawn(|| hub.run_shortcuts());
        scope.spawn(|| hub.run_music());
        keep_connected(&[&AdbLinks::default()], &context, &AtomicBool::new(false));
        hub.stop();
    });
    Ok(())
}

fn pair() -> Result<(), String> {
    let folder = store::folder().map_err(|e| e.0)?;
    let _lock = store::lock(&folder).map_err(|e| e.0)?;
    // 등록만 하므로 구독이 없고 수집도 돌리지 않는다. 날씨 설정(키체인)도 열지 않는다.
    let hub = collectors();
    let keys = FileKeys::new(folder);
    let links = AdbLinks::default();
    let phones: Vec<_> = links
        .attached()
        .map_err(|e| e.0)?
        .into_iter()
        .filter(|p| p.blocked.is_none())
        .collect();
    let phone = match phones.as_slice() {
        [phone] => phone,
        [] => return Err("연결된 기기가 없음".to_owned()),
        _ => return Err("기기를 하나만 꽂고 다시 실행".to_owned()),
    };
    let mut conn = links.connect(&phone.id).map_err(|e| e.0)?;
    let context = Context {
        name: &Name,
        keys: &keys,
        prompt: &Terminal,
        report: &Terminal,
        requests: &PairRequests::default(),
        active: &Active::default(),
        hub: &hub,
        timing: Timing::default(),
    };
    match serve(
        phone,
        conn.as_mut(),
        &context,
        true,
        &AtomicBool::new(false),
    ) {
        Ok(Ended::Paired) => {
            println!("이제 sidedesk run 으로 연결하면 됩니다");
            Ok(())
        }
        Ok(_) => Err("등록 전에 연결이 끊김".to_owned()),
        Err(error) => Err(error.to_string()),
    }
}

/// 맥에서 `SideDesk.app/Contents/MacOS/` 안에서 돌고 있는지
#[cfg(target_os = "macos")]
fn in_app_bundle() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|dir| dir.ends_with("Contents/MacOS")))
        .unwrap_or(false)
}

/// 설정 창. 창이 열려 있는 동안 연결을 유지한다. 값들은 프로세스가 끝날 때까지 쓰므로 놓아주지 않는다.
fn window() -> Result<(), String> {
    let folder = store::folder().map_err(|e| e.0)?;
    let lock = store::lock(&folder).map_err(|e| e.0)?;
    let mode_file = folder.join("mode.txt");
    let (hub, prefs, help) = hub(&folder);
    let prefs: &'static WeatherPrefs = Box::leak(Box::new(prefs));
    let keys: &'static FileKeys = Box::leak(Box::new(FileKeys::new(folder)));
    let identity = keys.identity().map_err(|e| e.0)?;
    let requests: &'static PairRequests = Box::leak(Box::default());
    let computer = Computer {
        name: Name.read().map_err(|e| e.0)?,
        version: env!("CARGO_PKG_VERSION").to_owned(),
        key: identity.public,
    };
    let board: &'static Board = Box::leak(Box::new(
        Board::new(computer, keys, requests).with_weather(prefs),
    ));
    let context: &'static Context = Box::leak(Box::new(Context {
        name: &Name,
        keys,
        prompt: board,
        report: board,
        requests,
        active: Box::leak(Box::default()),
        hub: Box::leak(Box::new(hub)),
        timing: Timing::default(),
    }));
    thread::spawn(|| context.hub.run());
    thread::spawn(|| context.hub.run_weather());
    thread::spawn(|| context.hub.run_ai());
    thread::spawn(|| context.hub.run_forge());
    thread::spawn(|| context.hub.run_shortcuts());
    thread::spawn(|| context.hub.run_music());
    if let Some(deck) = context.hub.shortcuts() {
        let panel = sidedesk_window::ShortcutsPanel::new(Arc::clone(deck), help);
        sidedesk_window::with_shortcuts(Box::leak(Box::new(panel)));
    }
    // 창을 닫으면 돌고 있는 gh, glab 을 끝내고 나간다.
    sidedesk_window::on_exit(|| context.hub.stop());
    thread::spawn(move || {
        let _lock = lock;
        let usb = AdbLinks::default();
        #[cfg(target_os = "macos")]
        if in_app_bundle() {
            let air = sidedesk_mac::BleLinks::start();
            keep_connected(&[&usb, &air], context, &AtomicBool::new(false));
            return;
        }
        // 맥 블루투스는 Info.plist 가 있는 SideDesk.app 안에서만 쓸 수 있다.
        #[cfg(target_os = "macos")]
        board.link(
            sidedesk_domain::Link::Ble,
            Some("블루투스를 쓰려면 SideDesk.app 으로 실행하세요".to_owned()),
        );
        keep_connected(&[&usb], context, &AtomicBool::new(false));
    });
    sidedesk_window::open(board, mode_file)
}
