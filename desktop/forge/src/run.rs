//! 정해진 인자로 외부 프로그램을 돌린다. 쉘을 거치지 않는다.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

/// 출력은 이만큼만 남긴다(나머지는 읽어서 버린다).
const KEEP: usize = 64 * 1024;

pub struct Ran {
    /// 없으면 실행하지 못했거나(없음) 시간이 넘어 끝냈다.
    pub code: Option<i32>,
    pub started: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

pub trait Runner: Send + Sync {
    fn run(&self, program: &Path, args: &[&str]) -> Ran;
    /// 도는 프로그램을 끝낸다.
    fn stop(&self) {}
}

pub struct System {
    timeout: Duration,
    home: PathBuf,
    child: Mutex<Option<Child>>,
}

impl System {
    pub fn new(home: PathBuf, timeout: Duration) -> Self {
        Self {
            timeout,
            home,
            child: Mutex::new(None),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Child>> {
        self.child.lock().unwrap_or_else(|lock| lock.into_inner())
    }
}

impl Runner for System {
    fn run(&self, program: &Path, args: &[&str]) -> Ran {
        let not_started = Ran {
            code: None,
            started: false,
            stdout: Vec::new(),
            stderr: Vec::new(),
        };
        // 작업 폴더가 저장소면 그 원격을 따라가지 않게 홈에서 돌린다.
        let spawned = Command::new(program)
            .args(args)
            .current_dir(&self.home)
            .env("GH_PROMPT_DISABLED", "1")
            .env("GH_NO_UPDATE_NOTIFIER", "1")
            .env("NO_COLOR", "1")
            .env("GLAB_CHECK_UPDATE", "false")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
        let Ok(mut child) = spawned else {
            return not_started;
        };
        // 파이프가 차서 막히지 않게 끝까지 읽는다.
        let out = child.stdout.take().map(drain);
        let err = child.stderr.take().map(drain);
        *self.lock() = Some(child);
        let start = Instant::now();
        let code = loop {
            let mut slot = self.lock();
            let Some(child) = slot.as_mut() else {
                // stop 이 끝냈다.
                break None;
            };
            match child.try_wait() {
                Ok(Some(status)) => {
                    slot.take();
                    break status.code();
                }
                Ok(None) if start.elapsed() < self.timeout => {}
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    slot.take();
                    break None;
                }
            }
            drop(slot);
            thread::sleep(Duration::from_millis(20));
        };
        Ran {
            code,
            started: true,
            stdout: out.and_then(|h| h.join().ok()).unwrap_or_default(),
            stderr: err.and_then(|h| h.join().ok()).unwrap_or_default(),
        }
    }

    fn stop(&self) {
        if let Some(mut child) = self.lock().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn drain(mut pipe: impl Read + Send + 'static) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut kept = Vec::new();
        let mut buffer = [0u8; 8192];
        while let Ok(n) = pipe.read(&mut buffer) {
            if n == 0 {
                break;
            }
            let room = KEEP.saturating_sub(kept.len());
            kept.extend_from_slice(&buffer[..n.min(room)]);
        }
        kept
    })
}

/// 앱(.app)은 PATH 가 짧아 Homebrew 자리를 먼저 본다.
pub fn locate(name: &str, home: &Path) -> Option<PathBuf> {
    let file = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    };
    let fixed = [
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        home.join(".local/bin"),
    ];
    let path = std::env::var_os("PATH").unwrap_or_default();
    fixed
        .into_iter()
        .chain(std::env::split_paths(&path))
        .map(|dir| dir.join(&file))
        .find(|candidate| candidate.is_file())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn system(timeout: Duration) -> System {
        System::new(std::env::temp_dir(), timeout)
    }

    #[test]
    fn big_output_does_not_block() {
        let ran = system(Duration::from_secs(10)).run(
            Path::new("/bin/sh"),
            &[
                "-c",
                "head -c 300000 /dev/zero | tr '\\0' 'a'; echo done >&2",
            ],
        );
        assert_eq!(ran.code, Some(0));
        assert_eq!(ran.stdout.len(), KEEP);
        assert_eq!(ran.stderr, b"done\n");
    }

    #[test]
    fn slow_programs_are_ended_and_reaped() {
        let start = Instant::now();
        let ran = system(Duration::from_millis(200)).run(Path::new("/bin/sleep"), &["5"]);
        assert_eq!(ran.code, None);
        assert!(ran.started);
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn stop_ends_a_running_program() {
        let runner = std::sync::Arc::new(system(Duration::from_secs(10)));
        let worker = {
            let runner = std::sync::Arc::clone(&runner);
            thread::spawn(move || runner.run(Path::new("/bin/sleep"), &["5"]))
        };
        thread::sleep(Duration::from_millis(200));
        let start = Instant::now();
        runner.stop();
        let ran = worker.join().unwrap();
        assert_eq!(ran.code, None);
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn missing_programs_do_not_start() {
        let ran = system(Duration::from_secs(1)).run(Path::new("/nonexistent/gh"), &[]);
        assert!(!ran.started);
    }

    #[test]
    fn fixed_places_come_before_path() {
        let home = std::env::temp_dir().join(format!("sidedesk-locate-{}", std::process::id()));
        std::fs::create_dir_all(home.join(".local/bin")).unwrap();
        std::fs::write(home.join(".local/bin/sidedesk-only-here"), b"").unwrap();
        assert_eq!(
            locate("sidedesk-only-here", &home),
            Some(home.join(".local/bin/sidedesk-only-here"))
        );
        assert_eq!(locate("sidedesk-nowhere", &home), None);
        let _ = std::fs::remove_dir_all(&home);
    }
}
