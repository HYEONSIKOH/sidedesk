use std::process::ExitCode;

use sidedesk_domain::{PhoneLinks, greet};
#[cfg(target_os = "macos")]
use sidedesk_mac::MacName as Name;
#[cfg(target_os = "windows")]
use sidedesk_windows::WindowsName as Name;

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        Some("hello") => hello(),
        _ => {
            eprintln!("사용: sidedesk hello");
            ExitCode::FAILURE
        }
    }
}

fn links() -> Vec<Box<dyn PhoneLinks>> {
    vec![
        #[cfg(feature = "dev-link")]
        Box::new(sidedesk_link::AdbLinks::default()),
        #[cfg(all(feature = "dev-link", target_os = "macos"))]
        Box::new(sidedesk_link::SimulatorLinks::default()),
    ]
}

fn hello() -> ExitCode {
    let links = links();
    if links.is_empty() {
        eprintln!("개발용 연결이 없는 빌드");
        return ExitCode::FAILURE;
    }
    let links: Vec<_> = links.iter().map(Box::as_ref).collect();
    let outcomes = match greet(&Name, &links) {
        Ok(outcomes) => outcomes,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    if outcomes.is_empty() {
        eprintln!("연결된 폰이 없음");
    }
    for outcome in &outcomes {
        match &outcome.result {
            Ok(()) => println!("{}: 보냄", outcome.target),
            Err(error) => eprintln!("{}: {error}", outcome.target),
        }
    }
    if outcomes.iter().any(|outcome| outcome.result.is_ok()) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
