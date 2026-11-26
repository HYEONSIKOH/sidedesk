use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use crate::port::{Note, PhoneLinks};
use crate::serve::{Context, Ended, serve};

pub const RETRY: Duration = Duration::from_secs(1);

/// 붙은 폰마다 스레드 하나를 두고 연결을 유지한다. `stop` 이 켜질 때까지 돌아오지 않는다.
pub fn keep_connected(links: &dyn PhoneLinks, context: &Context, stop: &AtomicBool) {
    thread::scope(|scope| {
        let mut workers: HashMap<String, Arc<AtomicBool>> = HashMap::new();
        while !stop.load(Ordering::Relaxed) {
            let watched = links.watch(&mut |phones| {
                let ready: HashSet<String> = phones
                    .iter()
                    .filter(|phone| phone.blocked.is_none())
                    .map(|phone| phone.id.clone())
                    .collect();
                workers.retain(|id, quit| {
                    let keep = ready.contains(id);
                    if !keep {
                        quit.store(true, Ordering::Relaxed);
                    }
                    keep
                });
                for phone in &phones {
                    if let Some(reason) = &phone.blocked {
                        context.report.note(&phone.id, Note::Lost(reason.clone()));
                    }
                }
                for id in ready {
                    workers.entry(id.clone()).or_insert_with(|| {
                        let quit = Arc::new(AtomicBool::new(false));
                        let flag = Arc::clone(&quit);
                        scope.spawn(move || keep_one(links, context, &id, &flag));
                        quit
                    });
                }
            });
            if let Err(error) = watched {
                context.report.note("adb", Note::Lost(error.0));
            }
            rest(stop);
        }
        for quit in workers.values() {
            quit.store(true, Ordering::Relaxed);
        }
    });
}

fn keep_one(links: &dyn PhoneLinks, context: &Context, id: &str, quit: &AtomicBool) {
    let mut last_failure = None;
    while !quit.load(Ordering::Relaxed) {
        match links.connect(id) {
            Ok(mut conn) => {
                last_failure = None;
                match serve(id, conn.as_mut(), context, false, quit) {
                    Ok(Ended::Stopped) => return,
                    Ok(_) => context.report.note(id, Note::Lost("끊김".to_owned())),
                    Err(error) if error.is_final() => {
                        context.report.note(id, Note::Refused(error.to_string()));
                        while !quit.load(Ordering::Relaxed) {
                            rest(quit);
                        }
                        return;
                    }
                    Err(error) => context.report.note(id, Note::Lost(error.to_string())),
                }
            }
            // 폰 앱이 꺼져 있으면 1초마다 다시 시도하므로, 같은 이유는 한 번만 알린다.
            Err(error) if last_failure.as_ref() != Some(&error) => {
                context.report.note(id, Note::Lost(error.0.clone()));
                last_failure = Some(error);
            }
            Err(_) => {}
        }
        rest(quit);
    }
}

fn rest(quit: &AtomicBool) {
    let step = Duration::from_millis(50);
    let mut waited = Duration::ZERO;
    while waited < RETRY && !quit.load(Ordering::Relaxed) {
        thread::sleep(step);
        waited += step;
    }
}
