mod store;

use std::io::BufRead;
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self, Receiver};
use std::thread;

use sidedesk_domain::{
    Context, Ended, Note, PhoneLinks, Prompt, Report, Timing, keep_connected, serve,
};
use sidedesk_link::AdbLinks;
#[cfg(target_os = "macos")]
use sidedesk_mac::MacName as Name;
#[cfg(target_os = "windows")]
use sidedesk_windows::WindowsName as Name;

use crate::store::FileKeys;

const USAGE: &str = "사용: sidedesk pair (폰과 짝 맺기) | sidedesk run (폰과 연결 유지)";

fn main() -> ExitCode {
    let command = std::env::args().nth(1);
    let result = match command.as_deref() {
        Some("pair") => pair(),
        Some("run") => run(),
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
            Note::Connected => println!("{phone}: 연결됨"),
            Note::Paired => println!("{phone}: 짝을 맺음"),
            Note::Lost(reason) => eprintln!("{phone}: {reason}"),
            Note::Refused(reason) => eprintln!("{phone}: {reason}. 폰을 다시 꽂을 때까지 기다림"),
        }
    }
}

impl Prompt for Terminal {
    fn ask(&self, _phone: &str, code: &str) -> Receiver<bool> {
        println!("폰에 뜬 숫자가 {code} 이면 y, 아니면 n 을 입력하고 엔터");
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

fn run() -> Result<(), String> {
    let folder = store::folder().map_err(|e| e.0)?;
    let _lock = store::lock(&folder).map_err(|e| e.0)?;
    let keys = FileKeys::new(folder);
    let context = Context {
        name: &Name,
        keys: &keys,
        prompt: &Terminal,
        report: &Terminal,
        timing: Timing::default(),
    };
    println!("폰을 기다리는 중. 끝내려면 Ctrl+C");
    keep_connected(&AdbLinks::default(), &context, &AtomicBool::new(false));
    Ok(())
}

fn pair() -> Result<(), String> {
    let folder = store::folder().map_err(|e| e.0)?;
    let _lock = store::lock(&folder).map_err(|e| e.0)?;
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
        [] => return Err("연결된 폰이 없음".to_owned()),
        _ => return Err("폰을 하나만 꽂고 다시 실행".to_owned()),
    };
    let mut conn = links.connect(&phone.id).map_err(|e| e.0)?;
    let context = Context {
        name: &Name,
        keys: &keys,
        prompt: &Terminal,
        report: &Terminal,
        timing: Timing::default(),
    };
    match serve(
        &phone.id,
        conn.as_mut(),
        &context,
        true,
        &AtomicBool::new(false),
    ) {
        Ok(Ended::Paired) => {
            println!("이제 sidedesk run 으로 연결하면 됩니다");
            Ok(())
        }
        Ok(_) => Err("짝을 맺기 전에 연결이 끊김".to_owned()),
        Err(error) => Err(error.to_string()),
    }
}
