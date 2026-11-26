//! 음악: 지금 곡, 앨범 그림, 볼륨을 보는 폰에 보내고 폰의 조작을 실행한다.
//! 아무도 안 보면 감시를 끊는다. 곡 제목은 로그나 오류 문장에 남기지 않는다.

use std::collections::{HashMap, HashSet, VecDeque};
use std::panic::{self, AssertUnwindSafe};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sidedesk_core::{
    Artwork, CommandResult, MusicAction, MusicCommand, MusicPlayer, NowPlaying, Volume,
};

use crate::port::PortError;

/// 한 장 상한(Envelope 와 암호 꼬리를 더해도 한 프레임 안)
pub const ART_LIMIT: usize = 60 * 1024;
const TEXT_LEN: usize = 200;
const KEEP_PICTURES: usize = 8;
const REMEMBERED: usize = 256;
/// 예측 위치와 이만큼 넘게 다르면 바뀐 것으로 본다.
const DRIFT: Duration = Duration::from_millis(1500);

#[derive(Clone, Copy, Debug)]
pub struct MusicCadence {
    /// 재생 중 다시 맞추기
    pub playing: Duration,
    /// 멈춤 다시 맞추기
    pub paused: Duration,
    /// 볼륨 읽기
    pub volume: Duration,
}

impl Default for MusicCadence {
    fn default() -> Self {
        Self {
            playing: Duration::from_secs(15),
            paused: Duration::from_secs(60),
            volume: Duration::from_secs(1),
        }
    }
}

/// 플레이어에서 읽은 곡. `key` 는 곡이 바뀌었는지 보는 열쇠(보내지 않음).
#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub player: MusicPlayer,
    pub key: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: u64,
    pub playing: bool,
    pub position_ms: u64,
    pub measured_at: SystemTime,
    pub can_play: bool,
    pub can_next: bool,
    pub can_previous: bool,
}

impl Track {
    pub fn nothing() -> Self {
        Self {
            player: MusicPlayer::None,
            key: String::new(),
            title: String::new(),
            artist: String::new(),
            album: String::new(),
            duration_ms: 0,
            playing: false,
            position_ms: 0,
            measured_at: SystemTime::now(),
            can_play: false,
            can_next: false,
            can_previous: false,
        }
    }

    /// `at` 에서의 위치. 재생 중일 때만 흐르고 길이에서 멈춘다.
    pub fn position_at(&self, at: SystemTime) -> u64 {
        if !self.playing {
            return self.position_ms;
        }
        let gone = at
            .duration_since(self.measured_at)
            .map_or(0, |d| d.as_millis() as u64);
        let position = self.position_ms.saturating_add(gone);
        if self.duration_ms > 0 {
            position.min(self.duration_ms)
        } else {
            position
        }
    }

    /// 위치 말고 다른 것이 같고, 위치도 예측과 1.5초 안이면 같은 것으로 본다.
    fn same_as(&self, other: &Self) -> bool {
        let predicted = self.position_at(other.measured_at);
        self.player == other.player
            && self.key == other.key
            && self.title == other.title
            && self.artist == other.artist
            && self.album == other.album
            && self.duration_ms == other.duration_ms
            && self.playing == other.playing
            && self.can_play == other.can_play
            && self.can_next == other.can_next
            && self.can_previous == other.can_previous
            && predicted.abs_diff(other.position_ms) <= DRIFT.as_millis() as u64
    }
}

/// 줄여 다시 담은 그림
#[derive(Clone, Debug, PartialEq)]
pub struct Picture {
    pub hash: u64,
    pub jpeg: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Level {
    pub level: u32,
    pub muted: bool,
    pub settable: bool,
}

pub type Changed = Arc<dyn Fn() + Send + Sync>;

/// 재생 정보와 조작(맥은 음악 앱, Spotify)
pub trait Player: Send + Sync {
    /// 지금 곡. 곡이 바뀌었으면 그 그림도. 재생 중인 것이 없으면 player = None.
    fn now(&self) -> Result<(Track, Option<Picture>), PortError>;
    fn command(&self, action: MusicAction) -> Result<(), PortError>;
    /// 바뀌면 부를 함수. None 이면 감시를 끊는다.
    fn watch(&self, changed: Option<Changed>);
    /// 돌고 있는 외부 프로그램을 끝낸다.
    fn stop(&self) {}
}

/// 기본 출력 장치의 볼륨
pub trait Speaker: Send + Sync {
    /// 장치가 없으면 없음
    fn read(&self) -> Option<Level>;
    fn set(&self, level: u32) -> Result<(), PortError>;
}

#[derive(Default)]
struct Want {
    main: bool,
}

#[derive(Default)]
struct Outbox {
    track: Option<Track>,
    art: Option<Artwork>,
    volume: Option<Volume>,
    /// 이 연결에 보낸 그림 해시(마지막 8장)
    sent: VecDeque<u64>,
    results: VecDeque<CommandResult>,
}

struct Queued {
    command: MusicCommand,
    waiters: Vec<u64>,
}

#[derive(Default)]
struct Inner {
    wants: HashMap<u64, Want>,
    connected: HashSet<u64>,
    out: HashMap<u64, Outbox>,
    track: Option<Track>,
    pictures: VecDeque<Picture>,
    volume: Option<Volume>,
    queue: VecDeque<Queued>,
    done: HashMap<u64, CommandResult>,
    done_order: VecDeque<u64>,
    dirty: bool,
    watching: bool,
    stopped: bool,
    read_at: Option<Instant>,
    volume_at: Option<Instant>,
}

pub struct Jukebox {
    player: Box<dyn Player>,
    speaker: Box<dyn Speaker>,
    cadence: MusicCadence,
    inner: Arc<Mutex<Inner>>,
    changed: Arc<Condvar>,
}

fn millis(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

fn cut(text: &str) -> String {
    text.chars().take(TEXT_LEN).collect()
}

fn result(request_id: u64, outcome: Result<(), String>) -> CommandResult {
    CommandResult {
        request_id,
        ok: outcome.is_ok(),
        reason: outcome.err().unwrap_or_default(),
    }
}

impl Jukebox {
    pub fn new(player: Box<dyn Player>, speaker: Box<dyn Speaker>, cadence: MusicCadence) -> Self {
        Self {
            player,
            speaker,
            cadence,
            inner: Arc::default(),
            changed: Arc::new(Condvar::new()),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|l| l.into_inner())
    }

    pub fn join(&self, id: u64) {
        let mut inner = self.lock();
        inner.connected.insert(id);
        inner.out.entry(id).or_default();
    }

    /// `watching` 은 main 이나 tiles 에 음악이 있는지, `main` 은 큰 화면인지
    pub fn set_wants(&self, id: u64, watching: bool, main: bool) {
        let mut inner = self.lock();
        let before = inner.wants.get(&id).map(|w| w.main);
        if !watching {
            inner.wants.remove(&id);
            if let Some(out) = inner.out.get_mut(&id) {
                out.track = None;
                out.art = None;
                out.volume = None;
            }
        } else {
            inner.wants.insert(id, Want { main });
            // 처음 보거나 큰 화면이 되면 들고 있는 것을 바로 준다.
            if before.is_none() || (main && before == Some(false)) {
                let track = inner.track.clone();
                let volume = inner.volume;
                let picture = track
                    .as_ref()
                    .and_then(|_| Self::picture_of(&inner).cloned());
                let out = inner.out.entry(id).or_default();
                out.track = track;
                if main {
                    out.volume = volume;
                    if let Some(picture) = picture {
                        Self::offer_art(out, &picture);
                    }
                }
            }
        }
        drop(inner);
        self.changed.notify_all();
    }

    pub fn leave(&self, id: u64) {
        let mut inner = self.lock();
        inner.wants.remove(&id);
        inner.connected.remove(&id);
        inner.out.remove(&id);
        for queued in &mut inner.queue {
            queued.waiters.retain(|w| *w != id);
        }
        drop(inner);
        self.changed.notify_all();
    }

    /// 폰이 보낸 조작. 줄에 넣고 음악 스레드가 하나씩 한다.
    pub fn enqueue(&self, subscriber: u64, command: &MusicCommand) {
        let mut inner = self.lock();
        let action = MusicAction::try_from(command.action).unwrap_or(MusicAction::Unknown);
        let reject = match action {
            MusicAction::Unknown => Some("모르는 조작입니다"),
            MusicAction::SetVolume if command.level > 100 => Some("볼륨은 0~100 입니다"),
            _ if inner.stopped => Some("프로그램을 끄는 중이라 실행하지 않았습니다"),
            _ => None,
        };
        if let Some(reason) = reject {
            Self::deliver(
                &mut inner,
                subscriber,
                result(command.request_id, Err(reason.into())),
            );
            return;
        }
        // 볼륨은 여러 번 해도 같아 두 번 실행 막기 표에 넣지 않는다.
        if action != MusicAction::SetVolume {
            if let Some(done) = inner.done.get(&command.request_id).cloned() {
                Self::deliver(&mut inner, subscriber, done);
                return;
            }
            if let Some(queued) = inner
                .queue
                .iter_mut()
                .find(|q| q.command.request_id == command.request_id)
            {
                queued.waiters.push(subscriber);
                return;
            }
        }
        inner.queue.push_back(Queued {
            command: *command,
            waiters: vec![subscriber],
        });
        drop(inner);
        self.changed.notify_all();
    }

    /// 보낼 것: 곡(보내는 순간 위치로 다시 찍음), 그림, 볼륨, 명령 결과 순서
    pub fn take(
        &self,
        id: u64,
    ) -> (
        Option<NowPlaying>,
        Option<Artwork>,
        Option<Volume>,
        Vec<CommandResult>,
    ) {
        let mut inner = self.lock();
        let Some(out) = inner.out.get_mut(&id) else {
            return (None, None, None, Vec::new());
        };
        let now = SystemTime::now();
        let playing = out.track.take().map(|track| NowPlaying {
            player: track.player as i32,
            title: cut(&track.title),
            artist: cut(&track.artist),
            album: cut(&track.album),
            duration_ms: track.duration_ms,
            artwork: 0,
            playing: track.playing,
            position_ms: track.position_at(now),
            measured_at: millis(now),
            can_play: track.can_play,
            can_next: track.can_next,
            can_previous: track.can_previous,
        });
        let art = out.art.take();
        let volume = out.volume.take();
        let results = out.results.drain(..).collect();
        let track_art = Self::picture_of(&inner).map_or(0, |p| p.hash);
        let playing = playing.map(|mut n| {
            n.artwork = track_art;
            n
        });
        (playing, art, volume, results)
    }

    /// 지금 곡의 그림(곡이 바뀌면 비운다)
    fn picture_of(inner: &Inner) -> Option<&Picture> {
        inner.pictures.back()
    }

    fn offer_art(out: &mut Outbox, picture: &Picture) {
        if out.sent.contains(&picture.hash) {
            return;
        }
        if out.sent.len() >= KEEP_PICTURES {
            out.sent.pop_front();
        }
        out.sent.push_back(picture.hash);
        out.art = Some(Artwork {
            hash: picture.hash,
            jpeg: picture.jpeg.clone(),
        });
    }

    fn deliver(inner: &mut Inner, subscriber: u64, result: CommandResult) {
        if inner.connected.contains(&subscriber) {
            inner
                .out
                .entry(subscriber)
                .or_default()
                .results
                .push_back(result);
        }
    }

    fn remember(inner: &mut Inner, done: CommandResult) {
        if inner.done_order.len() >= REMEMBERED
            && let Some(old) = inner.done_order.pop_front()
        {
            inner.done.remove(&old);
        }
        inner.done_order.push_back(done.request_id);
        inner.done.insert(done.request_id, done);
    }

    /// 새로 읽은 곡과 그림을 들고, 바뀌었으면 보는 구독자에게 넣는다.
    fn publish_track(&self, inner: &mut Inner, track: Track, picture: Option<Picture>) {
        let changed_song = inner.track.as_ref().is_none_or(|old| old.key != track.key);
        if changed_song {
            inner.pictures.clear();
        }
        if let Some(picture) = picture.filter(|p| !p.jpeg.is_empty() && p.jpeg.len() <= ART_LIMIT) {
            if inner.pictures.len() >= KEEP_PICTURES {
                inner.pictures.pop_front();
            }
            inner.pictures.push_back(Picture {
                hash: picture.hash.max(1),
                jpeg: picture.jpeg,
            });
        }
        let same = inner.track.as_ref().is_some_and(|old| old.same_as(&track));
        inner.track = Some(track.clone());
        let picture = inner.pictures.back().cloned();
        let wants: Vec<(u64, bool)> = inner.wants.iter().map(|(id, w)| (*id, w.main)).collect();
        for (id, main) in wants {
            let out = inner.out.entry(id).or_default();
            if !same {
                out.track = Some(track.clone());
            }
            if main && let Some(picture) = &picture {
                Self::offer_art(out, picture);
                if out.art.is_some() && out.track.is_none() {
                    out.track = Some(track.clone());
                }
            }
        }
    }

    fn publish_volume(&self, inner: &mut Inner, level: Option<Level>) {
        let volume = match level {
            Some(l) => Volume {
                available: true,
                level: l.level.min(100),
                muted: l.muted,
                settable: l.settable,
            },
            None => Volume::default(),
        };
        if inner.volume == Some(volume) {
            return;
        }
        inner.volume = Some(volume);
        let mains: Vec<u64> = inner
            .wants
            .iter()
            .filter(|(_, w)| w.main)
            .map(|(id, _)| *id)
            .collect();
        for id in mains {
            inner.out.entry(id).or_default().volume = Some(volume);
        }
    }

    /// 보는 사람이 있는 동안 곡과 볼륨을 따라간다. `stop` 을 부를 때까지 돌아오지 않는다.
    pub fn run(&self) {
        let mut inner = self.lock();
        loop {
            if inner.stopped {
                return;
            }
            let watching = !inner.wants.is_empty();
            if watching != inner.watching {
                inner.watching = watching;
                drop(inner);
                if watching {
                    let shared = Arc::clone(&self.inner);
                    let changed = Arc::clone(&self.changed);
                    self.player.watch(Some(Arc::new(move || {
                        shared.lock().unwrap_or_else(|l| l.into_inner()).dirty = true;
                        changed.notify_all();
                    })));
                } else {
                    self.player.watch(None);
                }
                inner = self.lock();
                inner.dirty |= watching;
                continue;
            }
            if let Some(queued) = inner.queue.pop_front() {
                self.execute(inner, queued);
                inner = self.lock();
                continue;
            }
            if !watching {
                inner = self.changed.wait(inner).unwrap_or_else(|l| l.into_inner());
                continue;
            }
            let again = match &inner.track {
                Some(track) if track.playing => self.cadence.playing,
                _ => self.cadence.paused,
            };
            let due_track = inner.dirty || inner.read_at.is_none_or(|at| at.elapsed() >= again);
            let due_volume = inner
                .volume_at
                .is_none_or(|at| at.elapsed() >= self.cadence.volume);
            if due_track {
                inner.dirty = false;
                inner.read_at = Some(Instant::now());
                drop(inner);
                let read = panic::catch_unwind(AssertUnwindSafe(|| self.player.now()));
                inner = self.lock();
                if let Ok(Ok((track, picture))) = read {
                    self.publish_track(&mut inner, track, picture);
                }
                continue;
            }
            if due_volume {
                inner.volume_at = Some(Instant::now());
                drop(inner);
                let level =
                    panic::catch_unwind(AssertUnwindSafe(|| self.speaker.read())).unwrap_or(None);
                inner = self.lock();
                self.publish_volume(&mut inner, level);
                continue;
            }
            let wait = self.cadence.volume.min(again);
            inner = self
                .changed
                .wait_timeout(inner, wait)
                .unwrap_or_else(|l| l.into_inner())
                .0;
        }
    }

    fn execute(&self, mut inner: MutexGuard<'_, Inner>, queued: Queued) {
        let action = MusicAction::try_from(queued.command.action).unwrap_or(MusicAction::Unknown);
        if action == MusicAction::SetVolume {
            // 밀린 볼륨 명령 중 마지막만 적용하고 앞엣것은 ok 로 끝낸다.
            let mut last = queued;
            while let Some(index) = inner
                .queue
                .iter()
                .position(|q| MusicAction::try_from(q.command.action) == Ok(MusicAction::SetVolume))
            {
                let next = inner.queue.remove(index).expect("있음");
                let skipped = std::mem::replace(&mut last, next);
                for waiter in skipped.waiters {
                    Self::deliver(
                        &mut inner,
                        waiter,
                        result(skipped.command.request_id, Ok(())),
                    );
                }
            }
            drop(inner);
            let outcome =
                panic::catch_unwind(AssertUnwindSafe(|| self.speaker.set(last.command.level)))
                    .unwrap_or_else(|_| Err(PortError::new("실행하지 못했습니다")))
                    .map_err(|e| e.0);
            let level = self.speaker.read();
            let mut inner = self.lock();
            self.publish_volume(&mut inner, level);
            for waiter in last.waiters {
                Self::deliver(
                    &mut inner,
                    waiter,
                    result(last.command.request_id, outcome.clone()),
                );
            }
            return;
        }
        drop(inner);
        let outcome = panic::catch_unwind(AssertUnwindSafe(|| self.player.command(action)))
            .unwrap_or_else(|_| Err(PortError::new("실행하지 못했습니다")))
            .map_err(|e| e.0);
        let read = self.player.now();
        let mut inner = self.lock();
        if let Ok((track, picture)) = read {
            inner.read_at = Some(Instant::now());
            self.publish_track(&mut inner, track, picture);
        }
        let done = result(queued.command.request_id, outcome);
        Self::remember(&mut inner, done.clone());
        for waiter in queued.waiters {
            Self::deliver(&mut inner, waiter, done.clone());
        }
    }

    pub fn stop(&self) {
        self.lock().stopped = true;
        self.changed.notify_all();
        self.player.watch(None);
        self.player.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
    use std::thread;

    #[derive(Default)]
    struct Fake {
        reads: AtomicUsize,
        commands: AtomicUsize,
        watching: Mutex<Option<Changed>>,
        track: Mutex<Option<(Track, Option<Picture>)>>,
    }

    impl Player for Fake {
        fn now(&self) -> Result<(Track, Option<Picture>), PortError> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            Ok(self
                .track
                .lock()
                .unwrap()
                .clone()
                .unwrap_or((Track::nothing(), None)))
        }
        fn command(&self, _: MusicAction) -> Result<(), PortError> {
            self.commands.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        fn watch(&self, changed: Option<Changed>) {
            *self.watching.lock().unwrap() = changed;
        }
    }

    #[derive(Default)]
    struct Knob {
        level: AtomicU32,
        sets: AtomicUsize,
    }

    impl Speaker for Knob {
        fn read(&self) -> Option<Level> {
            Some(Level {
                level: self.level.load(Ordering::SeqCst),
                muted: false,
                settable: true,
            })
        }
        fn set(&self, level: u32) -> Result<(), PortError> {
            self.sets.fetch_add(1, Ordering::SeqCst);
            self.level.store(level, Ordering::SeqCst);
            Ok(())
        }
    }

    struct Shared<T>(Arc<T>);
    impl Player for Shared<Fake> {
        fn now(&self) -> Result<(Track, Option<Picture>), PortError> {
            self.0.now()
        }
        fn command(&self, a: MusicAction) -> Result<(), PortError> {
            self.0.command(a)
        }
        fn watch(&self, c: Option<Changed>) {
            self.0.watch(c)
        }
    }
    impl Speaker for Shared<Knob> {
        fn read(&self) -> Option<Level> {
            self.0.read()
        }
        fn set(&self, l: u32) -> Result<(), PortError> {
            self.0.set(l)
        }
    }

    fn song(key: &str, position_ms: u64, ago: Duration) -> Track {
        Track {
            player: MusicPlayer::Music,
            key: key.into(),
            title: "가".repeat(250),
            artist: "부른 이".into(),
            album: "앨범".into(),
            duration_ms: 200_000,
            playing: true,
            position_ms,
            measured_at: SystemTime::now() - ago,
            can_play: true,
            can_next: true,
            can_previous: true,
        }
    }

    fn picture(hash: u64, size: usize) -> Picture {
        Picture {
            hash,
            jpeg: vec![0xff; size],
        }
    }

    fn jukebox() -> (Arc<Jukebox>, Arc<Fake>, Arc<Knob>) {
        let fake = Arc::new(Fake::default());
        let knob = Arc::new(Knob::default());
        let cadence = MusicCadence {
            playing: Duration::from_millis(100),
            paused: Duration::from_millis(200),
            volume: Duration::from_millis(20),
        };
        (
            Arc::new(Jukebox::new(
                Box::new(Shared(fake.clone())),
                Box::new(Shared(knob.clone())),
                cadence,
            )),
            fake,
            knob,
        )
    }

    fn wait_until(done: impl Fn() -> bool) {
        for _ in 0..400 {
            if done() {
                return;
            }
            thread::sleep(Duration::from_millis(5));
        }
        panic!("기다려도 되지 않음");
    }

    #[test]
    fn positions_are_stamped_when_sent() {
        let track = song("a", 10_000, Duration::from_secs(15));
        assert!(track.position_at(SystemTime::now()) >= 24_900);
        let mut paused = track.clone();
        paused.playing = false;
        assert_eq!(paused.position_at(SystemTime::now()), 10_000);
        let mut end = track;
        end.position_ms = 199_000;
        assert_eq!(end.position_at(SystemTime::now()), 200_000);
    }

    #[test]
    fn watching_starts_and_stops_with_subscribers() {
        let (jukebox, fake, _) = jukebox();
        *fake.track.lock().unwrap() = Some((song("a", 0, Duration::ZERO), Some(picture(5, 100))));
        std::thread::scope(|scope| {
            scope.spawn(|| jukebox.run());
            thread::sleep(Duration::from_millis(50));
            assert!(fake.watching.lock().unwrap().is_none());
            assert_eq!(fake.reads.load(Ordering::SeqCst), 0);
            jukebox.join(1);
            jukebox.set_wants(1, true, true);
            wait_until(|| fake.watching.lock().unwrap().is_some());
            wait_until(|| fake.reads.load(Ordering::SeqCst) >= 1);
            jukebox.set_wants(1, false, false);
            wait_until(|| fake.watching.lock().unwrap().is_none());
            let reads = fake.reads.load(Ordering::SeqCst);
            thread::sleep(Duration::from_millis(300));
            assert_eq!(fake.reads.load(Ordering::SeqCst), reads);
            jukebox.stop();
        });
    }

    #[test]
    fn main_gets_song_then_art_and_volume_tiles_get_song_only() {
        let (jukebox, fake, knob) = jukebox();
        knob.level.store(30, Ordering::SeqCst);
        *fake.track.lock().unwrap() = Some((song("a", 0, Duration::ZERO), Some(picture(5, 100))));
        jukebox.join(1);
        jukebox.join(2);
        std::thread::scope(|scope| {
            scope.spawn(|| jukebox.run());
            jukebox.set_wants(1, true, true);
            jukebox.set_wants(2, true, false);
            wait_until(|| fake.reads.load(Ordering::SeqCst) >= 1);
            thread::sleep(Duration::from_millis(60));
            let (now, art, volume, _) = jukebox.take(1);
            let now = now.unwrap();
            assert_eq!(now.artwork, 5);
            assert_eq!(now.title.chars().count(), 200);
            assert_eq!(art.unwrap().hash, 5);
            assert_eq!(volume.unwrap().level, 30);
            let (now, art, volume, _) = jukebox.take(2);
            assert!(now.is_some() && art.is_none() && volume.is_none());
            // 같은 그림은 그 연결에 다시 보내지 않는다.
            fake.reads.store(0, Ordering::SeqCst);
            wait_until(|| fake.reads.load(Ordering::SeqCst) >= 1);
            let (_, art, _, _) = jukebox.take(1);
            assert!(art.is_none());
            jukebox.stop();
        });
    }

    #[test]
    fn a_new_subscriber_gets_the_position_of_now() {
        let (jukebox, _, _) = jukebox();
        {
            let mut inner = jukebox.lock();
            inner.track = Some(song("a", 10_000, Duration::from_secs(15)));
        }
        jukebox.join(1);
        jukebox.set_wants(1, true, false);
        let (now, _, _, _) = jukebox.take(1);
        assert!(now.unwrap().position_ms >= 24_900);
    }

    #[test]
    fn big_pictures_are_not_sent() {
        let (jukebox, _, _) = jukebox();
        jukebox.join(1);
        jukebox.set_wants(1, true, true);
        let mut inner = jukebox.lock();
        jukebox.publish_track(
            &mut inner,
            song("a", 0, Duration::ZERO),
            Some(picture(9, ART_LIMIT + 1)),
        );
        drop(inner);
        let (now, art, _, _) = jukebox.take(1);
        assert_eq!(now.unwrap().artwork, 0);
        assert!(art.is_none());
    }

    #[test]
    fn small_drift_is_not_resent() {
        let (jukebox, _, _) = jukebox();
        jukebox.join(1);
        jukebox.set_wants(1, true, false);
        let mut inner = jukebox.lock();
        let first = song("a", 10_000, Duration::from_secs(10));
        jukebox.publish_track(&mut inner, first, None);
        drop(inner);
        assert!(jukebox.take(1).0.is_some());
        let mut inner = jukebox.lock();
        // 예측(20초)과 0.5초 다름 → 같은 것
        jukebox.publish_track(&mut inner, song("a", 20_500, Duration::ZERO), None);
        drop(inner);
        assert!(jukebox.take(1).0.is_none());
        let mut inner = jukebox.lock();
        jukebox.publish_track(&mut inner, song("a", 60_000, Duration::ZERO), None);
        drop(inner);
        assert!(jukebox.take(1).0.is_some());
    }

    #[test]
    fn commands_once_volume_last_only_and_refusals() {
        let (jukebox, fake, knob) = jukebox();
        jukebox.join(1);
        let command = |request_id, action: MusicAction, level| MusicCommand {
            request_id,
            action: action as i32,
            level,
        };
        jukebox.enqueue(1, &command(1, MusicAction::PlayPause, 0));
        jukebox.enqueue(1, &command(1, MusicAction::PlayPause, 0));
        for (n, level) in [10, 20, 30, 40].iter().enumerate() {
            jukebox.enqueue(1, &command(10 + n as u64, MusicAction::SetVolume, *level));
        }
        jukebox.enqueue(1, &command(20, MusicAction::Unknown, 0));
        jukebox.enqueue(1, &command(21, MusicAction::SetVolume, 101));
        std::thread::scope(|scope| {
            scope.spawn(|| jukebox.run());
            wait_until(|| jukebox.lock().queue.is_empty());
            thread::sleep(Duration::from_millis(30));
            jukebox.stop();
        });
        let (_, _, _, results) = jukebox.take(1);
        assert_eq!(fake.commands.load(Ordering::SeqCst), 1);
        assert_eq!(knob.sets.load(Ordering::SeqCst), 1);
        assert_eq!(knob.level.load(Ordering::SeqCst), 40);
        // 볼륨 명령은 재생 정보를 다시 읽지 않는다(재생 명령 뒤 한 번만).
        assert!(fake.reads.load(Ordering::SeqCst) <= 1);
        let ids: Vec<u64> = results.iter().map(|r| r.request_id).collect();
        assert!(ids.contains(&20) && ids.contains(&21));
        assert!(results.iter().filter(|r| r.request_id == 1).count() == 2);
        assert!(!results.iter().find(|r| r.request_id == 21).unwrap().ok);
        assert!(
            results
                .iter()
                .filter(|r| (10..14).contains(&r.request_id))
                .all(|r| r.ok)
        );
    }
}
