use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use crate::port::{Note, Phone, PhoneLinks};
use crate::serve::{Context, Ended, serve};

pub const RETRY: Duration = Duration::from_secs(1);

/// 붙은 폰마다 스레드 하나를 두고 연결을 유지한다. `stop` 이 켜질 때까지 돌아오지 않는다.
pub fn keep_connected(links: &dyn PhoneLinks, context: &Context, stop: &AtomicBool) {
    thread::scope(|scope| {
        let mut workers: HashMap<String, Arc<AtomicBool>> = HashMap::new();
        while !stop.load(Ordering::Relaxed) {
            let watched = links.watch(&mut |phones| {
                context.report.phones(&phones);
                let ready: HashSet<String> = phones
                    .iter()
                    .filter(|phone| phone.blocked.is_none())
                    .map(|phone| phone.id.clone())
                    .collect();
                workers.retain(|id, quit| {
                    let keep = ready.contains(id);
                    if !keep {
                        quit.store(true, Ordering::Relaxed);
                        context.requests.take(id);
                    }
                    keep
                });
                for phone in &phones {
                    if let Some(reason) = &phone.blocked {
                        context.report.note(&phone.id, Note::Lost(reason.clone()));
                    }
                }
                for phone in phones.into_iter().filter(|phone| phone.blocked.is_none()) {
                    workers.entry(phone.id.clone()).or_insert_with(|| {
                        let quit = Arc::new(AtomicBool::new(false));
                        let flag = Arc::clone(&quit);
                        scope.spawn(move || {
                            keep_one(links, context, &phone, &flag);
                            context.requests.take(&phone.id);
                        });
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

fn keep_one(links: &dyn PhoneLinks, context: &Context, phone: &Phone, quit: &AtomicBool) {
    let id = phone.id.as_str();
    let mut last_failure = None;
    while !quit.load(Ordering::Relaxed) {
        // 요청은 접속이 된 뒤에 꺼낸다. 그 사이 창에서 짝 맺기를 그만두면 짝 맺기로 붙지 않는다.
        let pairing = context.requests.is_asked(id);
        match links.connect(id) {
            Ok(mut conn) => {
                last_failure = None;
                let pairing = pairing && context.requests.take(id);
                match serve(phone, conn.as_mut(), context, pairing, quit) {
                    Ok(Ended::Stopped) => return,
                    // 짝을 맺었으니 저장한 키로 바로 다시 붙는다.
                    Ok(Ended::Paired) => continue,
                    Ok(Ended::Closed) => context.report.note(id, Note::Lost("끊김".to_owned())),
                    Err(error) if error.is_final() => {
                        context.report.note(id, Note::Refused(error.to_string()));
                        // 다시 꽂거나 짝 맺기를 누를 때까지 기다린다.
                        while !quit.load(Ordering::Relaxed) && !context.requests.is_asked(id) {
                            rest(quit);
                        }
                        continue;
                    }
                    Err(error) => context.report.note(id, Note::Lost(error.to_string())),
                }
            }
            // 폰 앱이 꺼져 있으면 1초마다 다시 시도하므로, 같은 이유는 한 번만 알린다.
            Err(error) => {
                if last_failure.as_ref() != Some(&error) {
                    context.report.note(id, Note::Lost(error.0.clone()));
                    last_failure = Some(error);
                }
            }
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
