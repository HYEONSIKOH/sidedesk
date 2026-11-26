use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use sidedesk_core::{KEY_LEN, beacon};

use crate::port::{Link, Note, Phone, PhoneLinks};
use crate::serve::{Context, Ended, ServeError, serve};

pub const RETRY: Duration = Duration::from_secs(1);
/// 다른 컴퓨터가 쓰고 있어 거절됐을 때 다시 시도하는 간격
pub const BUSY_RETRY: Duration = Duration::from_secs(5);

/// 링크마다 기기 목록을 지켜보고, 기기마다 스레드 하나를 두고 연결을 유지한다.
/// `stop` 이 켜질 때까지 돌아오지 않는다.
pub fn keep_connected(links: &[&dyn PhoneLinks], context: &Context, stop: &AtomicBool) {
    thread::scope(|scope| {
        for &link in links {
            scope.spawn(move || watch_link(link, context, stop));
        }
    });
}

fn watch_link(links: &dyn PhoneLinks, context: &Context, stop: &AtomicBool) {
    let link = links.link();
    thread::scope(|scope| {
        let mut workers: HashMap<String, Arc<AtomicBool>> = HashMap::new();
        while !stop.load(Ordering::Relaxed) {
            let watched = links.watch(&mut |phones| {
                context.report.link(link, None);
                context.report.phones(link, &phones);
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
                            match phone.link {
                                Link::Usb => keep_usb(links, context, &phone, &flag),
                                Link::Ble => keep_ble(links, context, &phone, &flag),
                            }
                            context.requests.take(&phone.id);
                        });
                        quit
                    });
                }
            });
            if let Err(error) = watched {
                context.report.link(link, Some(error.0));
                // 블루투스를 쓸 수 없으면 기기 작업을 멈춘다(헛된 붙기 시도가 줄의 이유를 덮지 않게).
                // 다시 켜지면 새로 시작한다. USB 는 adb 가 잠깐 끊겨도 작업과 등록 요청을 남긴다.
                if link == Link::Ble {
                    for quit in workers.values() {
                        quit.store(true, Ordering::Relaxed);
                    }
                    workers.clear();
                }
            }
            rest(stop, RETRY);
        }
        for quit in workers.values() {
            quit.store(true, Ordering::Relaxed);
        }
    });
}

fn keep_usb(links: &dyn PhoneLinks, context: &Context, phone: &Phone, quit: &AtomicBool) {
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
                    Ok(Ended::Yielded) => {}
                    Ok(Ended::Closed) => context.report.note(id, Note::Lost("끊김".to_owned())),
                    Err(error) if error.is_final() => {
                        context.report.note(id, Note::Refused(error.to_string()));
                        // 다시 꽂거나 짝 맺기를 누를 때까지 기다린다.
                        while !quit.load(Ordering::Relaxed) && !context.requests.is_asked(id) {
                            rest(quit, RETRY);
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
        rest(quit, RETRY);
    }
}

/// 등록된 키 중 알림 신호 지문이 맞는 것
pub fn registered_key(context: &Context, phone: &Phone) -> Option<[u8; KEY_LEN]> {
    let mark = phone.beacon?;
    context
        .keys
        .trusted()
        .ok()?
        .into_iter()
        .map(|paired| paired.key)
        .find(|key| beacon(key) == mark)
}

fn trusted_keys(context: &Context) -> Vec<[u8; KEY_LEN]> {
    context
        .keys
        .trusted()
        .map(|list| list.into_iter().map(|paired| paired.key).collect())
        .unwrap_or_default()
}

/// 블루투스 기기 하나. 등록 확인과 USB 확인을 시도할 때마다 한다. 짝 맺기는 하지 않는다.
fn keep_ble(links: &dyn PhoneLinks, context: &Context, phone: &Phone, quit: &AtomicBool) {
    let id = phone.id.as_str();
    let mut last_failure = None;
    while !quit.load(Ordering::Relaxed) {
        let Some(key) = registered_key(context, phone) else {
            rest(quit, RETRY);
            continue;
        };
        // USB 로 붙어 있거나, 주소가 바뀌기 전 블루투스 연결이 아직 살아 있으면 기다린다.
        if context.active.holds(key, Link::Usb) || context.active.holds(key, Link::Ble) {
            rest(quit, RETRY);
            continue;
        }
        let mut wait = RETRY;
        match links.connect(id) {
            Ok(mut conn) => {
                last_failure = None;
                match serve(phone, conn.as_mut(), context, false, quit) {
                    Ok(Ended::Stopped) => return,
                    Ok(Ended::Paired | Ended::Yielded) => {}
                    Ok(Ended::Closed) => context.report.note(id, Note::Lost("끊김".to_owned())),
                    Err(ServeError::Busy) => {
                        context
                            .report
                            .note(id, Note::Lost(ServeError::Busy.to_string()));
                        wait = BUSY_RETRY;
                    }
                    Err(error) if error.is_final() => {
                        context.report.note(id, Note::Refused(error.to_string()));
                        // 블루투스에는 짝 맺기 요청이 없으므로, 등록 목록이 바뀔 때까지 기다린다.
                        let before = trusted_keys(context);
                        while !quit.load(Ordering::Relaxed) && trusted_keys(context) == before {
                            rest(quit, RETRY);
                        }
                        continue;
                    }
                    Err(error) => context.report.note(id, Note::Lost(error.to_string())),
                }
            }
            Err(error) => {
                // 이미 멈춘 작업(사라진 주소)은 알리지 않는다.
                if !quit.load(Ordering::Relaxed) && last_failure.as_ref() != Some(&error) {
                    context.report.note(id, Note::Lost(error.0.clone()));
                    last_failure = Some(error);
                }
            }
        }
        rest(quit, wait);
    }
}

fn rest(quit: &AtomicBool, time: Duration) {
    let step = Duration::from_millis(50);
    let mut waited = Duration::ZERO;
    while waited < time && !quit.load(Ordering::Relaxed) {
        thread::sleep(step);
        waited += step;
    }
}
