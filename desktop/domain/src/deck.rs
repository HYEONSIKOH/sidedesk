//! 단축키 장부: 목록 보관과 고치기, 구독자에게 보내기, 실행과 두 번 실행 막기.
//! 실행은 따로 스레드에서 하고, 5초 안에 안 끝나면 "보냈음"으로 먼저 돌려준다.

use std::collections::{HashMap, HashSet, VecDeque};
use std::panic::{self, AssertUnwindSafe};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use sidedesk_core::{CommandResult, RunShortcut, ShortcutItem, ShortcutToggle, Shortcuts};

use crate::port::PortError;
use crate::shortcut::{Action, Launcher, Saved, Shortcut, ShortcutStore, SystemAction};

/// 이만큼 안에 결과가 안 오면 "보냈음"
pub const ANSWER_WITHIN: Duration = Duration::from_secs(5);
/// 동시에 도는 실행 수
pub const MAX_RUNNING: usize = 4;
/// 켜짐 상태를 다시 묻는 간격
pub const TOGGLE_EVERY: Duration = Duration::from_secs(2);
const REMEMBERED: usize = 256;

struct Pending {
    subscribers: Vec<u64>,
    started: Instant,
}

#[derive(Default)]
struct Inner {
    saved: Option<Saved>,
    /// 불러올 때 되돌렸거나 저장에 실패한 까닭(설정 창에 보임)
    notice: Option<String>,
    toggles: HashMap<u32, bool>,
    wants: HashSet<u64>,
    connected: HashSet<u64>,
    outbox: HashMap<u64, Shortcuts>,
    last_list: Option<Shortcuts>,
    results: HashMap<u64, VecDeque<CommandResult>>,
    pending: HashMap<u64, Pending>,
    done: HashMap<u64, CommandResult>,
    done_order: VecDeque<u64>,
    running: usize,
    stopped: bool,
}

pub struct Deck {
    store: Box<dyn ShortcutStore>,
    launcher: Arc<dyn Launcher>,
    inner: Mutex<Inner>,
    changed: Condvar,
}

fn done(request_id: u64, outcome: Result<(), String>) -> CommandResult {
    CommandResult {
        request_id,
        ok: outcome.is_ok(),
        reason: outcome.err().unwrap_or_default(),
    }
}

impl Deck {
    /// 처음 쓸 때 불러온다. 불러오지 못하면 빈 목록과 알림.
    pub fn new(store: Box<dyn ShortcutStore>, launcher: Arc<dyn Launcher>) -> Arc<Self> {
        Arc::new(Self {
            store,
            launcher,
            inner: Mutex::default(),
            changed: Condvar::new(),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        let mut inner = self.inner.lock().unwrap_or_else(|lock| lock.into_inner());
        if inner.saved.is_none() {
            let (mut saved, notice) = match self.store.load() {
                Ok(loaded) => loaded,
                Err(error) => (Saved::new(0), Some(error.0)),
            };
            let repaired = saved.repair();
            inner.notice = notice.or_else(|| {
                repaired.then(|| "같은 번호의 단축키가 있어 뒤엣것을 뺐습니다".to_owned())
            });
            inner.saved = Some(saved);
        }
        inner
    }

    // 설정 창 쪽

    pub fn items(&self) -> Vec<Shortcut> {
        self.lock()
            .saved
            .as_ref()
            .map(|s| s.items.clone())
            .unwrap_or_default()
    }

    pub fn notice(&self) -> Option<String> {
        self.lock().notice.clone()
    }

    pub fn can_send_keys(&self) -> bool {
        self.launcher.can_send_keys()
    }

    /// 새 번호를 주고 끝에 더한다.
    pub fn add(&self, mut shortcut: Shortcut) -> Result<u32, String> {
        shortcut.tidy().map_err(str::to_owned)?;
        self.change(|saved| {
            shortcut.id = saved.next_id;
            saved.next_id += 1;
            saved.items.push(shortcut.clone());
            Ok(shortcut.id)
        })
    }

    pub fn update(&self, mut shortcut: Shortcut) -> Result<(), String> {
        shortcut.tidy().map_err(str::to_owned)?;
        self.change(|saved| {
            let slot = saved
                .items
                .iter_mut()
                .find(|item| item.id == shortcut.id)
                .ok_or("없는 단축키입니다")?;
            *slot = shortcut.clone();
            Ok(())
        })
    }

    pub fn remove(&self, id: u32) -> Result<(), String> {
        self.change(|saved| {
            let before = saved.items.len();
            saved.items.retain(|item| item.id != id);
            if before == saved.items.len() {
                return Err("없는 단축키입니다".to_owned());
            }
            Ok(())
        })
    }

    /// 한 칸 위(-1)나 아래(+1). 끝에서는 그대로.
    pub fn shift(&self, id: u32, step: i32) -> Result<(), String> {
        self.change(|saved| {
            let at = saved
                .items
                .iter()
                .position(|item| item.id == id)
                .ok_or("없는 단축키입니다")?;
            let to = at as i64 + i64::from(step.signum());
            if (0..saved.items.len() as i64).contains(&to) {
                saved.items.swap(at, to as usize);
            }
            Ok(())
        })
    }

    /// 저장에 성공했을 때만 메모리를 바꾸고 폰에 보낸다.
    fn change<T>(&self, edit: impl FnOnce(&mut Saved) -> Result<T, String>) -> Result<T, String> {
        let mut inner = self.lock();
        let mut next = inner.saved.clone().unwrap_or_else(|| Saved::new(0));
        let value = edit(&mut next)?;
        if let Err(error) = self.store.save(&next) {
            let text = format!("단축키를 저장하지 못했습니다: {}", error.0);
            inner.notice = Some(text.clone());
            return Err(text);
        }
        inner.notice = None;
        inner.saved = Some(next);
        self.publish(&mut inner);
        drop(inner);
        self.changed.notify_all();
        Ok(value)
    }

    // 구독자 쪽

    pub fn join(&self, id: u64) {
        self.lock().connected.insert(id);
    }

    pub fn set_wants(&self, id: u64, wants: bool) {
        let mut inner = self.lock();
        if wants && inner.wants.insert(id) {
            let list = self.list(&inner);
            inner.outbox.insert(id, list);
        } else if !wants {
            inner.wants.remove(&id);
            inner.outbox.remove(&id);
        }
        drop(inner);
        self.changed.notify_all();
    }

    pub fn leave(&self, id: u64) {
        let mut inner = self.lock();
        inner.wants.remove(&id);
        inner.connected.remove(&id);
        inner.outbox.remove(&id);
        inner.results.remove(&id);
        for pending in inner.pending.values_mut() {
            pending.subscribers.retain(|s| *s != id);
        }
    }

    /// 보낼 목록과 명령 결과. 5초 넘게 도는 명령은 여기서 "보냈음"으로 끝낸다.
    pub fn take(&self, id: u64) -> (Option<Shortcuts>, Vec<CommandResult>) {
        let mut inner = self.lock();
        let late: Vec<u64> = inner
            .pending
            .iter()
            .filter(|(_, p)| p.started.elapsed() >= ANSWER_WITHIN)
            .map(|(request_id, _)| *request_id)
            .collect();
        for request_id in late {
            Self::finish_locked(&mut inner, request_id, done(request_id, Ok(())));
        }
        let list = inner.outbox.remove(&id);
        let results = inner
            .results
            .get_mut(&id)
            .map(|queue| queue.drain(..).collect())
            .unwrap_or_default();
        (list, results)
    }

    /// 폰이 보낸 실행 명령
    pub fn run(self: &Arc<Self>, subscriber: u64, command: &RunShortcut) {
        let mut inner = self.lock();
        let request_id = command.request_id;
        if let Some(result) = inner.done.get(&request_id).cloned() {
            Self::deliver(&mut inner, subscriber, result);
            return;
        }
        if let Some(pending) = inner.pending.get_mut(&request_id) {
            pending.subscribers.push(subscriber);
            return;
        }
        let saved = inner.saved.as_ref().expect("불러옴");
        let refuse = |reason: &str| done(request_id, Err(reason.to_owned()));
        let outcome = if command.book != saved.book {
            Err(refuse(
                "다른 컴퓨터의 단축키입니다. 목록을 다시 받는 중입니다",
            ))
        } else if let Some(shortcut) = saved.items.iter().find(|s| s.id == command.shortcut_id) {
            match shortcut.action.check() {
                Err(reason) => Err(refuse(reason)),
                Ok(()) if inner.running >= MAX_RUNNING => {
                    Err(refuse("이미 여러 개를 실행 중이라 실행하지 못했습니다"))
                }
                Ok(()) => Ok(shortcut.action.clone()),
            }
        } else {
            Err(refuse("없는 단축키입니다"))
        };
        let action = match outcome {
            Ok(action) => action,
            Err(result) => {
                inner.pending.insert(
                    request_id,
                    Pending {
                        subscribers: vec![subscriber],
                        started: Instant::now(),
                    },
                );
                Self::finish_locked(&mut inner, request_id, result);
                return;
            }
        };
        inner.running += 1;
        inner.pending.insert(
            request_id,
            Pending {
                subscribers: vec![subscriber],
                started: Instant::now(),
            },
        );
        drop(inner);
        let deck = Arc::clone(self);
        thread::spawn(move || {
            let outcome = panic::catch_unwind(AssertUnwindSafe(|| deck.launcher.run(&action)))
                .unwrap_or_else(|_| Err(PortError::new("실행하지 못했습니다")))
                .map_err(|error| error.0);
            let toggle = deck.launcher.toggle(&action);
            let mut inner = deck.lock();
            inner.running = inner.running.saturating_sub(1);
            Self::finish_locked(&mut inner, request_id, done(request_id, outcome));
            // 누른 직후 켜짐 상태를 바로 다시 보낸다.
            if let Some(on) = toggle {
                let ids: Vec<u32> = inner
                    .saved
                    .as_ref()
                    .map(|s| {
                        s.items
                            .iter()
                            .filter(|i| i.action == action)
                            .map(|i| i.id)
                            .collect()
                    })
                    .unwrap_or_default();
                for id in ids {
                    inner.toggles.insert(id, on);
                }
                deck.publish(&mut inner);
            }
        });
    }

    /// 도는 중 표에서 빼고 결과를 기억하고 받을 사람에게 준다. 이미 끝냈으면(5초 "보냈음") 버린다.
    fn finish_locked(inner: &mut Inner, request_id: u64, result: CommandResult) {
        let Some(pending) = inner.pending.remove(&request_id) else {
            return;
        };
        if inner.done_order.len() >= REMEMBERED
            && let Some(old) = inner.done_order.pop_front()
        {
            inner.done.remove(&old);
        }
        inner.done_order.push_back(request_id);
        inner.done.insert(request_id, result.clone());
        for subscriber in pending.subscribers {
            Self::deliver(inner, subscriber, result.clone());
        }
    }

    fn deliver(inner: &mut Inner, subscriber: u64, result: CommandResult) {
        if inner.connected.contains(&subscriber) {
            inner
                .results
                .entry(subscriber)
                .or_default()
                .push_back(result);
        }
    }

    fn list(&self, inner: &Inner) -> Shortcuts {
        let saved = inner.saved.as_ref().expect("불러옴");
        Shortcuts {
            book: saved.book,
            items: saved
                .items
                .iter()
                .map(|item| ShortcutItem {
                    id: item.id,
                    name: item.name.clone(),
                    icon: item.icon.clone(),
                    color: item.color.clone(),
                    toggle: match (has_toggle(&item.action), inner.toggles.get(&item.id)) {
                        (true, Some(true)) => ShortcutToggle::On,
                        (true, Some(false)) => ShortcutToggle::Off,
                        _ => ShortcutToggle::None,
                    } as i32,
                    system: item.action.is_system(),
                })
                .collect(),
        }
    }

    /// 목록이 바뀌었으면 보는 구독자 모두에게
    fn publish(&self, inner: &mut Inner) {
        let list = self.list(inner);
        if inner.last_list.as_ref() == Some(&list) {
            return;
        }
        let wants: Vec<u64> = inner.wants.iter().copied().collect();
        for id in wants {
            inner.outbox.insert(id, list.clone());
        }
        inner.last_list = Some(list);
    }

    /// 보는 구독자가 있고 켜짐 상태가 있는 단축키가 있는 동안 2초마다 상태를 묻는다.
    pub fn watch_toggles(&self, every: Duration) {
        let mut inner = self.lock();
        loop {
            if inner.stopped {
                return;
            }
            let actions: Vec<(u32, Action)> = inner
                .saved
                .as_ref()
                .map(|s| {
                    s.items
                        .iter()
                        .filter(|i| has_toggle(&i.action))
                        .map(|i| (i.id, i.action.clone()))
                        .collect()
                })
                .unwrap_or_default();
            if inner.wants.is_empty() || actions.is_empty() {
                inner = self
                    .changed
                    .wait(inner)
                    .unwrap_or_else(|lock| lock.into_inner());
                continue;
            }
            drop(inner);
            let states: Vec<(u32, Option<bool>)> = actions
                .iter()
                .map(|(id, action)| (*id, self.launcher.toggle(action)))
                .collect();
            inner = self.lock();
            for (id, state) in states {
                match state {
                    Some(on) => inner.toggles.insert(id, on),
                    None => inner.toggles.remove(&id),
                };
            }
            self.publish(&mut inner);
            inner = self
                .changed
                .wait_timeout(inner, every)
                .unwrap_or_else(|lock| lock.into_inner())
                .0;
        }
    }

    pub fn stop(&self) {
        self.lock().stopped = true;
        self.changed.notify_all();
        self.launcher.stop();
    }
}

fn has_toggle(action: &Action) -> bool {
    matches!(
        action,
        Action::System {
            action: SystemAction::Mute | SystemAction::Focus { .. }
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shortcut::Combo;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[derive(Default)]
    struct Memory {
        saved: Mutex<Option<Saved>>,
        fail: AtomicBool,
    }

    impl ShortcutStore for Memory {
        fn load(&self) -> Result<(Saved, Option<String>), PortError> {
            Ok((
                self.saved
                    .lock()
                    .unwrap()
                    .clone()
                    .unwrap_or_else(|| Saved::new(77)),
                None,
            ))
        }
        fn save(&self, saved: &Saved) -> Result<(), PortError> {
            if self.fail.load(Ordering::SeqCst) {
                return Err(PortError::new("디스크 가득"));
            }
            *self.saved.lock().unwrap() = Some(saved.clone());
            Ok(())
        }
    }

    #[derive(Default)]
    struct Fake {
        runs: AtomicUsize,
        slow: AtomicBool,
        muted: AtomicBool,
    }

    impl Launcher for Fake {
        fn run(&self, action: &Action) -> Result<(), PortError> {
            self.runs.fetch_add(1, Ordering::SeqCst);
            if self.slow.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_millis(300));
            }
            if matches!(
                action,
                Action::System {
                    action: SystemAction::Mute
                }
            ) {
                self.muted.fetch_xor(true, Ordering::SeqCst);
            }
            Ok(())
        }
        fn toggle(&self, action: &Action) -> Option<bool> {
            matches!(
                action,
                Action::System {
                    action: SystemAction::Mute
                }
            )
            .then(|| self.muted.load(Ordering::SeqCst))
        }
        fn can_send_keys(&self) -> bool {
            true
        }
    }

    fn url(name: &str) -> Shortcut {
        Shortcut {
            id: 0,
            name: name.into(),
            icon: "browser".into(),
            color: "orange".into(),
            action: Action::Url {
                url: "https://example.com".into(),
            },
        }
    }

    fn mute() -> Shortcut {
        Shortcut {
            id: 0,
            name: "소리".into(),
            icon: "volume".into(),
            color: "sand".into(),
            action: Action::System {
                action: SystemAction::Mute,
            },
        }
    }

    fn deck() -> (Arc<Deck>, Arc<Fake>) {
        let fake = Arc::new(Fake::default());
        (Deck::new(Box::new(Memory::default()), fake.clone()), fake)
    }

    fn command(request_id: u64, shortcut_id: u32) -> RunShortcut {
        RunShortcut {
            request_id,
            shortcut_id,
            book: 77,
        }
    }

    fn wait_results(deck: &Deck, id: u64) -> Vec<CommandResult> {
        for _ in 0..200 {
            let (_, results) = deck.take(id);
            if !results.is_empty() {
                return results;
            }
            thread::sleep(Duration::from_millis(5));
        }
        panic!("결과가 오지 않음");
    }

    #[test]
    fn ids_never_come_back() {
        let (deck, _) = deck();
        let a = deck.add(url("a")).unwrap();
        let b = deck.add(url("b")).unwrap();
        deck.remove(b).unwrap();
        let c = deck.add(url("c")).unwrap();
        assert_eq!((a, b, c), (1, 2, 3));
        assert!(deck.remove(b).is_err());
        assert!(deck.update(Shortcut { id: b, ..url("x") }).is_err());
        assert!(deck.shift(b, 1).is_err());
    }

    #[test]
    fn shifting_at_the_ends_stays() {
        let (deck, _) = deck();
        let a = deck.add(url("a")).unwrap();
        let b = deck.add(url("b")).unwrap();
        deck.shift(a, -1).unwrap();
        deck.shift(b, 1).unwrap();
        let ids: Vec<u32> = deck.items().iter().map(|s| s.id).collect();
        assert_eq!(ids, [a, b]);
        deck.shift(a, 1).unwrap();
        let ids: Vec<u32> = deck.items().iter().map(|s| s.id).collect();
        assert_eq!(ids, [b, a]);
    }

    #[test]
    fn bad_registrations_and_failed_saves_change_nothing() {
        let fake = Arc::new(Fake::default());
        let memory = Box::new(Memory::default());
        let deck = Deck::new(memory, fake);
        let mut bad = url("x");
        bad.action = Action::Url {
            url: "file:///etc/hosts".into(),
        };
        assert!(deck.add(bad).is_err());
        assert!(deck.items().is_empty());
    }

    #[test]
    fn a_failed_save_keeps_memory_and_tells() {
        let memory = Memory::default();
        memory.fail.store(true, Ordering::SeqCst);
        let deck = Deck::new(Box::new(memory), Arc::new(Fake::default()));
        deck.join(1);
        deck.set_wants(1, true);
        deck.take(1);
        assert!(deck.add(url("a")).is_err());
        assert!(deck.items().is_empty());
        assert!(deck.notice().unwrap().contains("저장하지 못했습니다"));
        assert_eq!(deck.take(1).0, None);
    }

    #[test]
    fn runs_once_per_request_and_refuses_unknowns() {
        let (deck, fake) = deck();
        let id = deck.add(url("a")).unwrap();
        deck.join(1);
        deck.run(1, &command(10, id));
        assert!(wait_results(&deck, 1)[0].ok);
        deck.run(1, &command(10, id));
        assert!(wait_results(&deck, 1)[0].ok);
        assert_eq!(fake.runs.load(Ordering::SeqCst), 1);
        deck.run(1, &command(11, 999));
        assert_eq!(wait_results(&deck, 1)[0].reason, "없는 단축키입니다");
        deck.run(
            1,
            &RunShortcut {
                request_id: 12,
                shortcut_id: id,
                book: 1,
            },
        );
        assert!(wait_results(&deck, 1)[0].reason.contains("다른 컴퓨터"));
        assert_eq!(fake.runs.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_hand_edited_url_is_checked_again_before_running() {
        let memory = Memory::default();
        let mut saved = Saved::new(77);
        saved.items.push(Shortcut {
            id: 4,
            action: Action::Url {
                url: "javascript:alert(1)".into(),
            },
            ..url("x")
        });
        saved.next_id = 5;
        *memory.saved.lock().unwrap() = Some(saved);
        let fake = Arc::new(Fake::default());
        let deck = Deck::new(Box::new(memory), fake.clone());
        deck.join(1);
        deck.run(1, &command(1, 4));
        assert!(!wait_results(&deck, 1)[0].ok);
        assert_eq!(fake.runs.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_duplicate_while_running_gets_the_same_answer() {
        let (deck, fake) = deck();
        fake.slow.store(true, Ordering::SeqCst);
        let id = deck.add(url("a")).unwrap();
        deck.join(1);
        deck.join(2);
        deck.run(1, &command(20, id));
        deck.run(2, &command(20, id));
        assert!(wait_results(&deck, 1)[0].ok);
        assert!(wait_results(&deck, 2)[0].ok);
        assert_eq!(fake.runs.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn too_many_at_once_are_refused() {
        let (deck, fake) = deck();
        fake.slow.store(true, Ordering::SeqCst);
        let id = deck.add(url("a")).unwrap();
        deck.join(1);
        for n in 0..=MAX_RUNNING as u64 {
            deck.run(1, &command(100 + n, id));
        }
        let (_, results) = deck.take(1);
        assert_eq!(results.len(), 1);
        assert!(results[0].reason.contains("여러 개"));
    }

    #[test]
    fn list_goes_out_when_it_changes_and_toggles_follow() {
        let (deck, fake) = deck();
        let id = deck.add(mute()).unwrap();
        deck.join(1);
        deck.set_wants(1, true);
        let (list, _) = deck.take(1);
        let list = list.unwrap();
        assert_eq!(list.book, 77);
        assert!(list.items[0].system);
        assert_eq!(list.items[0].toggle, ShortcutToggle::None as i32);
        thread::scope(|scope| {
            scope.spawn(|| deck.watch_toggles(Duration::from_millis(20)));
            for _ in 0..100 {
                if let (Some(list), _) = deck.take(1) {
                    assert_eq!(list.items[0].toggle, ShortcutToggle::Off as i32);
                    break;
                }
                thread::sleep(Duration::from_millis(5));
            }
            // 누르면 바로 켜짐으로
            deck.run(1, &command(30, id));
            wait_results(&deck, 1);
            assert!(fake.muted.load(Ordering::SeqCst));
            deck.stop();
        });
        // 보지 않는 구독자에게는 목록을 보내지 않는다.
        deck.join(2);
        deck.add(url("b")).unwrap();
        assert_eq!(deck.take(2).0, None);
    }

    #[test]
    fn a_slow_run_answers_sent_after_five_seconds() {
        let (deck, _) = deck();
        deck.join(1);
        {
            let mut inner = deck.lock();
            inner.pending.insert(
                40,
                Pending {
                    subscribers: vec![1],
                    started: Instant::now() - ANSWER_WITHIN - Duration::from_millis(1),
                },
            );
        }
        let (_, results) = deck.take(1);
        assert_eq!(results, [done(40, Ok(()))]);
        // 늦게 끝난 실제 결과는 버리고, 다시 보내도 "보냈음"
        Deck::finish_locked(&mut deck.lock(), 40, done(40, Err("늦음".into())));
        assert!(deck.take(1).1.is_empty());
        deck.run(1, &command(40, 1));
        assert_eq!(deck.take(1).1, [done(40, Ok(()))]);
    }

    #[test]
    fn keys_shortcuts_check_their_code() {
        let (deck, _) = deck();
        let keys = Shortcut {
            action: Action::Keys {
                combo: Combo {
                    code: "NotAKey".into(),
                    command: true,
                    control: false,
                    option: false,
                    shift: false,
                },
            },
            ..url("k")
        };
        assert!(deck.add(keys).is_err());
    }
}
