//! 정해진 인자로 외부 프로그램을 돌린다. 쉘을 거치지 않는다.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
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

/// 돌고 있는 자식. 윈도우는 손자까지 끝내려고 작업 개체에 넣는다.
struct Running {
    child: Child,
    #[cfg(windows)]
    job: Option<crate::job::Job>,
}

impl Running {
    fn new(child: Child) -> Self {
        Self {
            #[cfg(windows)]
            job: crate::job::Job::hold(&child),
            child,
        }
    }

    fn end(&mut self) {
        #[cfg(windows)]
        if let Some(job) = &self.job {
            job.kill();
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub struct System {
    timeout: Duration,
    home: PathBuf,
    child: Mutex<Option<Running>>,
    /// 끄는 중. 이 뒤의 실행은 바로 돌아온다.
    stopped: AtomicBool,
}

impl System {
    pub fn new(home: PathBuf, timeout: Duration) -> Self {
        Self {
            timeout,
            home,
            child: Mutex::new(None),
            stopped: AtomicBool::new(false),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Running>> {
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
        if self.stopped.load(Ordering::SeqCst) {
            // 끄는 중이면 새로 띄우지 않는다(실패로 본다).
            return Ran {
                started: true,
                ..not_started
            };
        }
        // 작업 폴더가 저장소면 그 원격을 따라가지 않게 홈에서 돌린다.
        let mut command = Command::new(program);
        // 창 프로그램(트레이 앱)이 콘솔 프로그램을 띄울 때 검은 창이 번쩍이지 않게 한다.
        #[cfg(windows)]
        std::os::windows::process::CommandExt::creation_flags(&mut command, CREATE_NO_WINDOW);
        let spawned = command
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
        let mut running = Running::new(child);
        {
            let mut slot = self.lock();
            // 띄우는 사이 stop 이 왔으면 바로 끝낸다.
            if self.stopped.load(Ordering::SeqCst) {
                running.end();
            } else {
                *slot = Some(running);
            }
        }
        let start = Instant::now();
        let code = loop {
            let mut slot = self.lock();
            let Some(running) = slot.as_mut() else {
                // stop 이 끝냈다.
                break None;
            };
            match running.child.try_wait() {
                Ok(Some(status)) => {
                    slot.take();
                    break status.code();
                }
                Ok(None) if start.elapsed() < self.timeout => {}
                _ => {
                    running.end();
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
        self.stopped.store(true, Ordering::SeqCst);
        if let Some(mut running) = self.lock().take() {
            running.end();
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

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 앱(.app)은 PATH 가 짧아 Homebrew 자리를 먼저 본다.
#[cfg(not(windows))]
fn fixed_places(home: &Path) -> Vec<PathBuf> {
    vec![
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        home.join(".local/bin"),
    ]
}

/// 윈도우 설치 자리(공식 설치 파일, winget, scoop). glab 자리는 확인하지 못해 PATH 에 기댄다.
#[cfg(windows)]
fn fixed_places(home: &Path) -> Vec<PathBuf> {
    let env = |name: &str| std::env::var_os(name).map(PathBuf::from);
    let mut places = Vec::new();
    if let Some(programs) = env("ProgramFiles") {
        places.push(programs.join("GitHub CLI"));
        places.push(programs.join("glab"));
    }
    if let Some(local) = env("LOCALAPPDATA") {
        places.push(local.join("Microsoft\\WinGet\\Links"));
    }
    places.push(home.join("scoop\\shims"));
    places
}

pub fn locate(name: &str, home: &Path) -> Option<PathBuf> {
    let file = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    };
    let fixed = fixed_places(home);
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
        // 5초짜리를 기다리지 않았는지만 본다. 느린 CI 서버를 생각해 넉넉히.
        assert!(start.elapsed() < Duration::from_secs(4));
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
        assert!(start.elapsed() < Duration::from_secs(3));
        // 끈 뒤에는 새로 띄우지 않는다.
        let start = Instant::now();
        let ran = runner.run(Path::new("/bin/sleep"), &["5"]);
        assert!(ran.started && ran.code.is_none());
        assert!(start.elapsed() < Duration::from_secs(3));
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

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    fn cmd() -> PathBuf {
        locate("cmd", Path::new("C:\\")).expect("PATH 의 cmd.exe")
    }

    fn system(timeout: Duration) -> System {
        System::new(std::env::temp_dir(), timeout)
    }

    #[test]
    fn runs_and_keeps_the_output() {
        let ran = system(Duration::from_secs(10)).run(&cmd(), &["/c", "echo", "sidedesk"]);
        assert_eq!(ran.code, Some(0));
        assert!(String::from_utf8_lossy(&ran.stdout).contains("sidedesk"));
    }

    #[test]
    fn slow_programs_and_their_children_are_ended() {
        // cmd 가 손자 ping 을 띄운다. 작업 개체로 손자까지 끝내야 파이프가 닫혀 시간 안에 돌아온다.
        let start = Instant::now();
        let ran =
            system(Duration::from_millis(500)).run(&cmd(), &["/c", "ping", "-n", "6", "127.0.0.1"]);
        assert_eq!(ran.code, None);
        assert!(
            start.elapsed() < Duration::from_secs(4),
            "{:?}",
            start.elapsed()
        );
    }

    #[test]
    fn stop_ends_a_running_program() {
        let runner = std::sync::Arc::new(system(Duration::from_secs(10)));
        let worker = {
            let runner = std::sync::Arc::clone(&runner);
            thread::spawn(move || runner.run(&cmd(), &["/c", "ping", "-n", "6", "127.0.0.1"]))
        };
        thread::sleep(Duration::from_millis(300));
        let start = Instant::now();
        runner.stop();
        assert_eq!(worker.join().unwrap().code, None);
        assert!(start.elapsed() < Duration::from_secs(3));
    }
}
