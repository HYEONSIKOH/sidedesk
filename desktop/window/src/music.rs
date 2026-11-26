//! 설정 창 일반 칸의 음악 안내. 음악 앱 자동화 권한이 없으면 까닭과 설정 열기 단추를 보인다.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::json;
use sidedesk_domain::Jukebox;

type Listener = Box<dyn Fn(String) + Send>;
type Opener = Box<dyn Fn() + Send + Sync>;

pub struct MusicPanel {
    jukebox: Arc<Jukebox>,
    open_settings: Opener,
    changed: Mutex<Option<Listener>>,
    last: Mutex<Option<String>>,
}

impl MusicPanel {
    pub fn new(jukebox: Arc<Jukebox>, open_settings: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            jukebox,
            open_settings: Box::new(open_settings),
            changed: Mutex::new(None),
            last: Mutex::new(None),
        }
    }

    pub fn on_change(&'static self, send: impl Fn(String) + Send + 'static) {
        *self.changed.lock().unwrap_or_else(|l| l.into_inner()) = Some(Box::new(send));
        // 까닭이 바뀌었을 때만 다시 그린다(3초마다 본다).
        thread::spawn(move || {
            loop {
                let now = self.jukebox.problem();
                let mut last = self.last.lock().unwrap_or_else(|l| l.into_inner());
                if *last != now {
                    *last = now;
                    drop(last);
                    self.publish();
                }
                thread::sleep(Duration::from_secs(3));
            }
        });
    }

    pub fn snapshot(&self) -> String {
        json!({ "problem": self.jukebox.problem() }).to_string()
    }

    fn publish(&self) {
        if let Some(send) = self
            .changed
            .lock()
            .unwrap_or_else(|l| l.into_inner())
            .as_ref()
        {
            send(self.snapshot());
        }
    }

    /// 이 칸의 명령이면 처리하고 true
    pub fn command(&self, body: &str) -> bool {
        if body.trim() == r#"{"music-settings":true}"# {
            (self.open_settings)();
            return true;
        }
        false
    }
}
