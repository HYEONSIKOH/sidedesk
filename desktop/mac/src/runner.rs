//! 외부 프로그램(단축어, osascript)을 쉘 없이 정해진 인자로 돌린다.
//! 시간이 넘으면 끝내고 거두며, `stop` 은 도는 것을 모두 끝내고 그 뒤로는 띄우지 않는다.

use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

/// 출력은 이만큼만 남긴다(나머지는 읽어서 버린다).
const KEEP: usize = 64 * 1024;

pub struct Runner {
    tool: PathBuf,
    home: PathBuf,
    limit: Duration,
    keep: usize,
    children: Mutex<HashMap<u32, Child>>,
    stopped: AtomicBool,
}

impl Runner {
    pub fn new(tool: PathBuf, home: PathBuf, limit: Duration) -> Self {
        Self {
            tool,
            home,
            limit,
            keep: KEEP,
            children: Mutex::default(),
            stopped: AtomicBool::new(false),
        }
    }

    /// 출력을 남길 크기를 바꾼다(기본 64KB).
    pub fn keeping(mut self, keep: usize) -> Self {
        self.keep = keep;
        self
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<u32, Child>> {
        self.children
            .lock()
            .unwrap_or_else(|lock| lock.into_inner())
    }

    pub fn stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    /// 성공했는지와 표준 출력. 끈 뒤거나 띄우지 못하면 없음.
    /// `capture` 가 아니면 출력을 받지 않는다(띄운 손자가 파이프를 물고 남아도 막히지 않게).
    pub fn run(&self, args: &[&str], capture: bool) -> Option<(bool, Vec<u8>)> {
        if self.stopped() {
            return None;
        }
        let mut child = Command::new(&self.tool)
            .args(args)
            .current_dir(&self.home)
            .stdin(Stdio::null())
            .stdout(if capture {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let pid = child.id();
        let keep = self.keep;
        let out = child.stdout.take().map(|mut pipe| {
            thread::spawn(move || {
                let mut kept = Vec::new();
                let mut buffer = [0u8; 8192];
                while let Ok(n) = pipe.read(&mut buffer) {
                    if n == 0 {
                        break;
                    }
                    let room = keep.saturating_sub(kept.len());
                    kept.extend_from_slice(&buffer[..n.min(room)]);
                }
                kept
            })
        });
        {
            let mut children = self.lock();
            children.insert(pid, child);
            // 띄우는 사이 stop 이 왔으면 바로 끝낸다.
            if self.stopped()
                && let Some(mut child) = children.remove(&pid)
            {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        let start = Instant::now();
        let ok = loop {
            let mut children = self.lock();
            let Some(child) = children.get_mut(&pid) else {
                break false;
            };
            match child.try_wait() {
                Ok(Some(status)) => {
                    children.remove(&pid);
                    break status.success();
                }
                Ok(None) if start.elapsed() < self.limit => {}
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    children.remove(&pid);
                    break false;
                }
            }
            drop(children);
            thread::sleep(Duration::from_millis(15));
        };
        let output = out.and_then(|h| h.join().ok()).unwrap_or_default();
        Some((ok, output))
    }

    pub fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        for (_, mut child) in self.lock().drain() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn slow_runs_end_and_stop_ends_running_ones() {
        let quick = Runner::new(
            PathBuf::from("/bin/sleep"),
            std::env::temp_dir(),
            Duration::from_millis(200),
        );
        let start = Instant::now();
        assert!(quick.run(&["5"], false).is_some_and(|(ok, _)| !ok));
        // 5초짜리를 기다리지 않았는지만 본다. 느린 CI 서버를 생각해 넉넉히.
        assert!(start.elapsed() < Duration::from_secs(4));
        let runner = Arc::new(Runner::new(
            PathBuf::from("/bin/sleep"),
            std::env::temp_dir(),
            Duration::from_secs(10),
        ));
        let worker = {
            let runner = Arc::clone(&runner);
            thread::spawn(move || runner.run(&["5"], false))
        };
        thread::sleep(Duration::from_millis(200));
        let start = Instant::now();
        runner.stop();
        let _ = worker.join().unwrap();
        assert!(start.elapsed() < Duration::from_secs(3));
        assert!(runner.run(&["1"], false).is_none());
    }

    #[test]
    fn output_is_captured_and_capped() {
        let runner = Runner::new(
            PathBuf::from("/bin/sh"),
            std::env::temp_dir(),
            Duration::from_secs(10),
        );
        let (ok, out) = runner
            .run(&["-c", "head -c 100000 /dev/zero | tr '\\\\0' 'a'"], true)
            .unwrap();
        assert!(ok);
        assert_eq!(out.len(), KEEP);
    }
}
