//! 구독과 수집. 폰 여러 대가 수집 하나를 함께 쓰고, 아무도 안 보면 수집을 멈춘다.

use std::collections::{HashMap, VecDeque};
use std::panic::{self, AssertUnwindSafe};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime};

use sidedesk_core::{
    Body, CommandResult, KillProcess, Location, Performance, Process, Processes, RunShortcut,
    Subscribe, View,
};

use crate::ai::{AiBook, AiCadence};
use crate::deck::{Deck, TOGGLE_EVERY};
use crate::port::{Forges, Meter, Place, PortError, ProcessTable, SessionLog, WeatherSource};
use crate::weather::{Book, WeatherCadence};

/// CPU 상위와 메모리 상위를 이만큼씩 골라 합친다.
const TOP: usize = 30;
const NAME_LEN: usize = 64;
/// 두 번 실행을 막으려고 기억하는 명령 수
const REMEMBERED: usize = 256;

/// 주기의 이만큼을 앞당겨 봐 준다. 수집이 조금 늦게 깨도 한 번 건너뛰지 않게 한다.
fn slack(every: Duration) -> Duration {
    every / 10
}

#[derive(Clone, Copy, Debug)]
pub struct Cadence {
    pub performance_main: Duration,
    pub performance_tile: Duration,
    pub processes_main: Duration,
    pub processes_tile: Duration,
}

impl Default for Cadence {
    fn default() -> Self {
        Self {
            performance_main: Duration::from_secs(1),
            performance_tile: Duration::from_secs(2),
            processes_main: Duration::from_secs(2),
            processes_tile: Duration::from_secs(6),
        }
    }
}

impl Cadence {
    fn every(&self, subscribe: &Subscribe, view: View) -> Option<Duration> {
        let (main, tile) = match view {
            View::Performance => (self.performance_main, self.performance_tile),
            View::Processes => (self.processes_main, self.processes_tile),
            View::Nothing | View::Weather | View::Ai | View::Shortcuts | View::Music => {
                return None;
            }
        };
        if subscribe.main == view as i32 {
            Some(main)
        } else if subscribe.tiles.contains(&(view as i32)) {
            Some(tile)
        } else {
            None
        }
    }
}

struct Want {
    every: Duration,
    sent: Option<Instant>,
}

/// 자료 한 가지(성능 또는 프로세스)의 구독자, 마지막 값, 보낼 값.
struct Feed<T> {
    wants: HashMap<u64, Want>,
    /// 보내지 못한 값. 새 값이 오면 덮어써 최신 값만 남긴다.
    outbox: HashMap<u64, T>,
    latest: Option<(Instant, T)>,
    /// 마지막으로 수집을 시도한 때. 실패해도 남겨 헛돌지 않게 한다.
    tried: Option<Instant>,
}

impl<T> Default for Feed<T> {
    fn default() -> Self {
        Self {
            wants: HashMap::new(),
            outbox: HashMap::new(),
            latest: None,
            tried: None,
        }
    }
}

impl<T: Clone> Feed<T> {
    fn set(&mut self, id: u64, every: Option<Duration>, now: Instant) {
        let Some(every) = every else {
            self.remove(id);
            return;
        };
        if let Some(want) = self.wants.get_mut(&id) {
            want.every = every;
            return;
        }
        // 처음 보는 구독자: 최근 값이 있으면 바로 주고, 없으면 바로 모으게 한다.
        let mut sent = None;
        match &self.latest {
            Some((at, value)) if now.duration_since(*at) < every => {
                self.outbox.insert(id, value.clone());
                sent = Some(*at);
            }
            _ => self.tried = None,
        }
        self.wants.insert(id, Want { every, sent });
    }

    fn remove(&mut self, id: u64) {
        self.wants.remove(&id);
        self.outbox.remove(&id);
    }

    fn every(&self) -> Option<Duration> {
        self.wants.values().map(|want| want.every).min()
    }

    /// 다음에 모을 때. 아무도 안 보면 없음.
    fn next(&self, now: Instant) -> Option<Instant> {
        let every = self.every()?;
        Some(self.tried.map_or(now, |tried| tried + every))
    }

    fn due(&self, now: Instant) -> bool {
        match (self.next(now), self.every()) {
            (Some(next), Some(every)) => now + slack(every) >= next,
            _ => false,
        }
    }

    fn tried(&mut self, now: Instant) {
        self.tried = Some(now);
    }

    fn publish(&mut self, now: Instant, value: T) {
        for (id, want) in &mut self.wants {
            if want
                .sent
                .is_none_or(|sent| now + slack(want.every) >= sent + want.every)
            {
                self.outbox.insert(*id, value.clone());
                want.sent = Some(now);
            }
        }
        self.latest = Some((now, value));
    }

    fn take(&mut self, id: u64) -> Option<T> {
        self.outbox.remove(&id)
    }
}

#[derive(Default)]
struct State {
    next_id: u64,
    performance: Feed<Performance>,
    processes: Feed<Processes>,
    weather: Book,
    ai: AiBook,
    stopped: bool,
}

impl State {
    fn next(&self, now: Instant) -> Option<Instant> {
        match (self.performance.next(now), self.processes.next(now)) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }
}

#[derive(Default)]
struct Done {
    order: VecDeque<u64>,
    results: HashMap<u64, CommandResult>,
}

type Wall = Box<dyn Fn() -> SystemTime + Send + Sync>;

pub struct Hub {
    meter: Box<dyn Meter>,
    table: Box<dyn ProcessTable>,
    cadence: Cadence,
    weather: Option<Box<dyn WeatherSource>>,
    weather_cadence: WeatherCadence,
    wall: Wall,
    sessions: Option<Box<dyn SessionLog>>,
    forges: Option<Box<dyn Forges>>,
    ai_cadence: AiCadence,
    deck: Option<Arc<Deck>>,
    state: Mutex<State>,
    changed: Condvar,
    /// 날씨 스레드만 깨운다. 성능 수집 주기를 흔들지 않게 따로 둔다.
    weather_changed: Condvar,
    /// AI 세션과 GitHub/GitLab 스레드를 깨운다.
    ai_changed: Condvar,
    done: Mutex<Done>,
}

impl Hub {
    pub fn new(meter: Box<dyn Meter>, table: Box<dyn ProcessTable>, cadence: Cadence) -> Self {
        Self {
            meter,
            table,
            cadence,
            weather: None,
            weather_cadence: WeatherCadence::default(),
            wall: Box::new(SystemTime::now),
            sessions: None,
            forges: None,
            ai_cadence: AiCadence::default(),
            deck: None,
            state: Mutex::default(),
            changed: Condvar::new(),
            weather_changed: Condvar::new(),
            ai_changed: Condvar::new(),
            done: Mutex::default(),
        }
    }

    /// 날씨를 받는다. 없으면 날씨 스레드는 바로 끝나고 기기에는 날씨가 가지 않는다.
    pub fn with_weather(mut self, source: Box<dyn WeatherSource>, cadence: WeatherCadence) -> Self {
        self.weather = Some(source);
        self.weather_cadence = cadence;
        self
    }

    /// AI 세션 기록과 GitHub/GitLab. 없으면 그 스레드는 바로 끝난다.
    pub fn with_ai(
        mut self,
        sessions: Box<dyn SessionLog>,
        forges: Option<Box<dyn Forges>>,
        cadence: AiCadence,
    ) -> Self {
        self.sessions = Some(sessions);
        self.forges = forges;
        self.ai_cadence = cadence;
        self
    }

    /// AI 를 보는 기기가 있는 동안 기록을 읽는다. `stop` 을 부를 때까지 돌아오지 않는다.
    pub fn run_ai(&self) {
        let Some(log) = &self.sessions else {
            return;
        };
        let cadence = &self.ai_cadence;
        let mut state = self.lock();
        loop {
            if state.stopped {
                return;
            }
            if !state.ai.watching() {
                state = self
                    .ai_changed
                    .wait(state)
                    .unwrap_or_else(|lock| lock.into_inner());
                continue;
            }
            drop(state);
            let facts = guarded(|| log.poll());
            state = self.lock();
            if let Ok(facts) = facts
                && state.ai.watching()
            {
                state.ai.apply(facts, (self.wall)(), cadence);
            }
            state = self
                .ai_changed
                .wait_timeout(state, cadence.poll)
                .unwrap_or_else(|lock| lock.into_inner())
                .0;
        }
    }

    /// AI 큰 화면을 보는 기기가 있는 동안 3분마다 GitHub/GitLab 수를 받는다.
    pub fn run_forge(&self) {
        let Some(forges) = &self.forges else {
            return;
        };
        let cadence = &self.ai_cadence;
        let mut state = self.lock();
        loop {
            if state.stopped {
                return;
            }
            let now = (self.wall)();
            if state.ai.forge_due(now, cadence) {
                state.ai.forge_tried(now);
                drop(state);
                let counts = panic::catch_unwind(AssertUnwindSafe(|| forges.counts()));
                state = self.lock();
                // 끄는 중에 끊긴 실행의 실패는 보내지 않는다.
                if let Ok(counts) = counts
                    && !state.stopped
                {
                    state.ai.forge_publish(counts, (self.wall)());
                }
                continue;
            }
            state = match state.ai.forge_wait(now, cadence) {
                None => self
                    .ai_changed
                    .wait(state)
                    .unwrap_or_else(|lock| lock.into_inner()),
                Some(wait) => {
                    self.ai_changed
                        .wait_timeout(state, wait)
                        .unwrap_or_else(|lock| lock.into_inner())
                        .0
                }
            };
        }
    }

    /// 단축키 장부. 없으면 단축키 목록을 보내지 않고 실행은 "없는 단축키"다.
    pub fn with_shortcuts(mut self, deck: Arc<Deck>) -> Self {
        self.deck = Some(deck);
        self
    }

    pub fn shortcuts(&self) -> Option<&Arc<Deck>> {
        self.deck.as_ref()
    }

    /// 소리 끄기 같은 켜짐 상태를 따라간다. `stop` 을 부를 때까지 돌아오지 않는다.
    pub fn run_shortcuts(&self) {
        if let Some(deck) = &self.deck {
            deck.watch_toggles(TOGGLE_EVERY);
        }
    }

    /// 시험에서 벽시계를 바꾼다.
    pub fn with_wall(mut self, wall: impl Fn() -> SystemTime + Send + Sync + 'static) -> Self {
        self.wall = Box::new(wall);
        self
    }

    /// 날씨를 원하는 기기가 있는 동안 좌표마다 받는다. `stop` 을 부를 때까지 돌아오지 않는다.
    pub fn run_weather(&self) {
        let Some(source) = &self.weather else {
            return;
        };
        let cadence = &self.weather_cadence;
        let mut state = self.lock();
        loop {
            if state.stopped {
                return;
            }
            let now = (self.wall)();
            state.weather.tidy(now, cadence);
            if let Some(place) = state.weather.due(now) {
                drop(state);
                let result = guarded(|| source.fetch(place));
                state = self.lock();
                state.weather.finish(place, result, (self.wall)(), cadence);
                continue;
            }
            state = match state.weather.wait(now, cadence) {
                None => self
                    .weather_changed
                    .wait(state)
                    .unwrap_or_else(|lock| lock.into_inner()),
                Some(wait) => {
                    self.weather_changed
                        .wait_timeout(state, wait)
                        .unwrap_or_else(|lock| lock.into_inner())
                        .0
                }
            };
        }
    }

    /// 연결 하나가 구독자가 된다. 연결이 끝나 놓으면 구독도 사라진다.
    pub fn join(&self) -> Subscriber<'_> {
        let mut state = self.lock();
        state.next_id += 1;
        let id = state.next_id;
        drop(state);
        if let Some(deck) = &self.deck {
            deck.join(id);
        }
        Subscriber {
            hub: self,
            id,
            unanswered: Mutex::default(),
        }
    }

    /// 구독이 있는 동안 모은다. `stop` 을 부를 때까지 돌아오지 않는다.
    pub fn run(&self) {
        let mut state = self.lock();
        loop {
            if state.stopped {
                return;
            }
            let now = Instant::now();
            let performance = state.performance.due(now);
            let processes = state.processes.due(now);
            if performance || processes {
                if performance {
                    state.performance.tried(now);
                }
                if processes {
                    state.processes.tried(now);
                }
                drop(state);
                let performance = performance.then(|| guarded(|| self.meter.performance()));
                let processes = processes.then(|| guarded(|| self.table.processes()).map(top));
                state = self.lock();
                let now = Instant::now();
                if let Some(Ok(value)) = performance {
                    state.performance.publish(now, value);
                }
                if let Some(Ok(value)) = processes {
                    state.processes.publish(now, value);
                }
                continue;
            }
            state = match state.next(now) {
                None => self
                    .changed
                    .wait(state)
                    .unwrap_or_else(|lock| lock.into_inner()),
                Some(next) => {
                    self.changed
                        .wait_timeout(state, next.saturating_duration_since(now))
                        .unwrap_or_else(|lock| lock.into_inner())
                        .0
                }
            };
        }
    }

    pub fn stop(&self) {
        self.lock().stopped = true;
        self.changed.notify_all();
        self.weather_changed.notify_all();
        self.ai_changed.notify_all();
        if let Some(forges) = &self.forges {
            forges.stop();
        }
        if let Some(deck) = &self.deck {
            deck.stop();
        }
    }

    /// 같은 번호는 한 번만 실행하고, 다시 오면 그때 결과를 그대로 돌려준다.
    pub fn kill(&self, command: &KillProcess) -> CommandResult {
        let mut done = self.done.lock().unwrap_or_else(|lock| lock.into_inner());
        if let Some(result) = done.results.get(&command.request_id) {
            return result.clone();
        }
        let outcome = if command.pid == 0 {
            Err("잘못된 프로세스 번호".to_owned())
        } else {
            self.table
                .kill(command.pid, command.started_at)
                .map_err(|error| error.0)
        };
        let result = CommandResult {
            request_id: command.request_id,
            ok: outcome.is_ok(),
            reason: outcome.err().unwrap_or_default(),
        };
        if done.order.len() >= REMEMBERED
            && let Some(old) = done.order.pop_front()
        {
            done.results.remove(&old);
        }
        done.order.push_back(command.request_id);
        done.results.insert(command.request_id, result.clone());
        result
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|lock| lock.into_inner())
    }
}

pub struct Subscriber<'a> {
    hub: &'a Hub,
    id: u64,
    /// 단축키 장부가 없을 때의 거절
    unanswered: Mutex<Vec<CommandResult>>,
}

impl Subscriber<'_> {
    /// 폰이 지금 보이는 것 전체를 알렸다. 앞 구독을 바꾼다.
    pub fn set(&self, subscribe: &Subscribe) {
        let cadence = &self.hub.cadence;
        let now = Instant::now();
        let mut state = self.hub.lock();
        state
            .performance
            .set(self.id, cadence.every(subscribe, View::Performance), now);
        state
            .processes
            .set(self.id, cadence.every(subscribe, View::Processes), now);
        let weather = View::Weather as i32;
        state.weather.set_wants(
            self.id,
            subscribe.main == weather || subscribe.tiles.contains(&weather),
        );
        let ai = View::Ai as i32;
        state.ai.set_wants(
            self.id,
            subscribe.main == ai || subscribe.tiles.contains(&ai),
            subscribe.main == ai,
        );
        drop(state);
        if let Some(deck) = &self.hub.deck {
            let shortcuts = View::Shortcuts as i32;
            deck.set_wants(
                self.id,
                subscribe.main == shortcuts || subscribe.tiles.contains(&shortcuts),
            );
        }
        self.hub.changed.notify_all();
        self.hub.weather_changed.notify_all();
        self.hub.ai_changed.notify_all();
    }

    /// 폰이 누른 단축키. 결과는 `take` 로 나간다.
    pub fn run_shortcut(&self, command: &RunShortcut) {
        match &self.hub.deck {
            Some(deck) => deck.run(self.id, command),
            None => self
                .unanswered
                .lock()
                .unwrap_or_else(|l| l.into_inner())
                .push(CommandResult {
                    request_id: command.request_id,
                    ok: false,
                    reason: "없는 단축키입니다".to_owned(),
                }),
        }
    }

    /// 폰의 위치. 범위 밖이거나 한쪽만 오면 위치 없음.
    pub fn place(&self, location: &Location) {
        self.hub
            .lock()
            .weather
            .set_place(self.id, Place::from_location(location));
        self.hub.weather_changed.notify_all();
    }

    /// 보낼 값. 쌓지 않고 가장 새 값만 준다.
    pub fn take(&self) -> Vec<Body> {
        let mut state = self.hub.lock();
        let performance = state.performance.take(self.id).map(Body::Performance);
        let processes = state.processes.take(self.id).map(Body::Processes);
        let weather = state.weather.take(self.id).map(Body::Weather);
        let (sessions, alerts, forge) = state.ai.take(self.id);
        drop(state);
        let (shortcuts, results) = match &self.hub.deck {
            Some(deck) => deck.take(self.id),
            None => (None, Vec::new()),
        };
        let unanswered: Vec<CommandResult> =
            std::mem::take(&mut *self.unanswered.lock().unwrap_or_else(|l| l.into_inner()));
        performance
            .into_iter()
            .chain(processes)
            .chain(weather)
            .chain(sessions.map(Body::AiSessions))
            .chain(alerts.into_iter().map(Body::AiAlert))
            .chain(forge.map(Body::Forge))
            .chain(shortcuts.map(Body::Shortcuts))
            .chain(
                results
                    .into_iter()
                    .chain(unanswered)
                    .map(Body::CommandResult),
            )
            .collect()
    }
}

impl Drop for Subscriber<'_> {
    fn drop(&mut self) {
        let mut state = self.hub.lock();
        state.performance.remove(self.id);
        state.processes.remove(self.id);
        state.weather.remove(self.id);
        state.ai.remove(self.id);
        drop(state);
        if let Some(deck) = &self.hub.deck {
            deck.leave(self.id);
        }
        self.hub.changed.notify_all();
        self.hub.weather_changed.notify_all();
        self.hub.ai_changed.notify_all();
    }
}

/// 수집기가 panic 해도 수집 스레드는 살아 다음 주기에 다시 시도한다.
fn guarded<T>(collect: impl FnOnce() -> Result<T, PortError>) -> Result<T, PortError> {
    panic::catch_unwind(AssertUnwindSafe(collect))
        .unwrap_or_else(|_| Err(PortError::new("수집기가 멈춤")))
}

/// CPU 상위와 메모리 상위를 합친 목록. 이름은 길이를 줄인다.
fn top(mut all: Vec<Process>) -> Processes {
    all.sort_by(|a, b| b.cpu.total_cmp(&a.cpu));
    let mut keep: Vec<bool> = (0..all.len()).map(|index| index < TOP).collect();
    let mut by_memory: Vec<usize> = (0..all.len()).collect();
    by_memory.sort_by(|&a, &b| all[b].memory.cmp(&all[a].memory));
    for index in by_memory.into_iter().take(TOP) {
        keep[index] = true;
    }
    let items = all
        .into_iter()
        .zip(keep)
        .filter(|(_, keep)| *keep)
        .map(|(mut process, _)| {
            if let Some((cut, _)) = process.name.char_indices().nth(NAME_LEN) {
                process.name.truncate(cut);
            }
            process
        })
        .collect();
    Processes { items }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    #[test]
    fn a_slow_subscriber_gets_every_other_sample() {
        let start = Instant::now();
        let mut feed = Feed::<u32>::default();
        feed.set(1, Some(ms(1000)), start);
        feed.set(2, Some(ms(2000)), start);
        assert_eq!(feed.every(), Some(ms(1000)));
        assert!(feed.due(start));
        let mut got = (Vec::new(), Vec::new());
        for second in 0..5u32 {
            let now = start + ms(1000 * u64::from(second) + 3);
            feed.tried(now);
            feed.publish(now, second);
            got.0.extend(feed.take(1));
            got.1.extend(feed.take(2));
        }
        assert_eq!(got.0, [0, 1, 2, 3, 4]);
        assert_eq!(got.1, [0, 2, 4]);
    }

    #[test]
    fn nobody_watching_means_no_collection() {
        let start = Instant::now();
        let mut feed = Feed::<u32>::default();
        assert_eq!(feed.next(start), None);
        feed.set(1, Some(ms(1000)), start);
        feed.set(1, None, start);
        assert_eq!(feed.next(start), None);
        assert!(!feed.due(start + ms(10_000)));
    }

    #[test]
    fn only_the_newest_value_waits() {
        let start = Instant::now();
        let mut feed = Feed::<u32>::default();
        feed.set(1, Some(ms(1000)), start);
        feed.publish(start, 1);
        feed.publish(start + ms(1000), 2);
        feed.publish(start + ms(2000), 3);
        assert_eq!(feed.take(1), Some(3));
        assert_eq!(feed.take(1), None);
    }

    #[test]
    fn a_new_subscriber_gets_a_fresh_value_at_once() {
        let start = Instant::now();
        let mut feed = Feed::<u32>::default();
        feed.set(1, Some(ms(1000)), start);
        feed.tried(start);
        feed.publish(start, 7);
        feed.take(1);
        feed.set(2, Some(ms(2000)), start + ms(500));
        assert_eq!(feed.take(2), Some(7));
        // 다음 수집은 원래 주기대로
        assert!(!feed.due(start + ms(500)));
        // 오래된 값뿐이면 바로 모은다.
        feed.set(3, Some(ms(1000)), start + ms(5000));
        assert_eq!(feed.take(3), None);
        assert!(feed.due(start + ms(5000)));
    }

    #[test]
    fn main_is_faster_than_a_tile() {
        let cadence = Cadence::default();
        let subscribe = Subscribe {
            main: View::Processes as i32,
            tiles: vec![View::Performance as i32, 99],
        };
        assert_eq!(
            cadence.every(&subscribe, View::Processes),
            Some(Duration::from_secs(2))
        );
        assert_eq!(
            cadence.every(&subscribe, View::Performance),
            Some(Duration::from_secs(2))
        );
        let tiles = Subscribe {
            main: 0,
            tiles: vec![View::Processes as i32],
        };
        assert_eq!(
            cadence.every(&tiles, View::Processes),
            Some(Duration::from_secs(6))
        );
        assert_eq!(cadence.every(&tiles, View::Performance), None);
    }

    #[test]
    fn top_keeps_heavy_cpu_and_heavy_memory() {
        let all = (0..100u32)
            .map(|pid| Process {
                pid,
                name: "가".repeat(100),
                cpu: pid as f32,
                memory: u64::from(1000 - pid),
                started_at: 0,
            })
            .collect();
        let items = top(all).items;
        assert_eq!(items.len(), 2 * TOP);
        assert!(items.iter().any(|p| p.pid == 99));
        assert!(items.iter().any(|p| p.pid == 0));
        assert!(!items.iter().any(|p| p.pid == 50));
        assert!(items.iter().all(|p| p.name.chars().count() == NAME_LEN));
    }
}
