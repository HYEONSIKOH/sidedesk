//! 구독, 수집 주기, 멈춤, 명령 중복을 가짜 수집기와 가짜 폰으로 시험한다.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use sidedesk_core::{
    Body, Event, Identity, KillProcess, Performance, Process, Role, Session, Subscribe, View,
};
use sidedesk_domain::{
    Active, Cadence, ComputerName, Conn, Context, Ended, Hub, Keys, Meter, PairRequests, Paired,
    Phone, PortError, ProcessTable, Prompt, Received, Report, ServeError, Timing, serve,
};

#[derive(Clone, Default)]
struct Counts {
    performance: Arc<AtomicUsize>,
    processes: Arc<AtomicUsize>,
    kills: Arc<AtomicUsize>,
}

struct Fake(Counts);

impl Meter for Fake {
    fn performance(&self) -> Result<Performance, PortError> {
        let n = self.0.performance.fetch_add(1, Ordering::SeqCst);
        Ok(Performance {
            cpu: n as f32,
            ..Default::default()
        })
    }
}

impl ProcessTable for Fake {
    fn processes(&self) -> Result<Vec<Process>, PortError> {
        self.0.processes.fetch_add(1, Ordering::SeqCst);
        Ok(vec![Process {
            pid: 42,
            name: "가짜".into(),
            cpu: 1.0,
            memory: 1,
            started_at: 1_000,
        }])
    }

    fn kill(&self, pid: u32, started_at: u64) -> Result<(), PortError> {
        self.0.kills.fetch_add(1, Ordering::SeqCst);
        if pid == 42 && started_at == 1_000 {
            Ok(())
        } else {
            Err(PortError::new("프로세스가 바뀌었습니다"))
        }
    }
}

/// 성능 큰 화면 40ms, 작은 칸 80ms, 프로세스 큰 화면 80ms, 작은 칸 240ms
fn cadence() -> Cadence {
    Cadence {
        performance_main: Duration::from_millis(40),
        performance_tile: Duration::from_millis(80),
        processes_main: Duration::from_millis(80),
        processes_tile: Duration::from_millis(240),
    }
}

fn hub(counts: &Counts) -> Hub {
    Hub::new(
        Box::new(Fake(counts.clone())),
        Box::new(Fake(counts.clone())),
        cadence(),
    )
}

fn watch(main: View, tiles: &[View]) -> Subscribe {
    Subscribe {
        main: main as i32,
        tiles: tiles.iter().map(|view| *view as i32).collect(),
    }
}

/// 시험이 실패해 풀려도 수집 스레드를 멈춘다.
struct Stopper<'a>(&'a Hub);

impl Drop for Stopper<'_> {
    fn drop(&mut self) {
        self.0.stop();
    }
}

fn count(counter: &AtomicUsize) -> usize {
    counter.load(Ordering::SeqCst)
}

#[test]
fn collects_only_while_someone_watches() {
    let counts = Counts::default();
    let hub = hub(&counts);
    thread::scope(|scope| {
        scope.spawn(|| hub.run());
        let _stopper = Stopper(&hub);
        thread::sleep(Duration::from_millis(150));
        assert_eq!(count(&counts.performance), 0);

        let phone = hub.join();
        phone.set(&watch(View::Performance, &[View::Processes]));
        let start = Instant::now();
        thread::sleep(Duration::from_millis(400));
        let performance = count(&counts.performance);
        let processes = count(&counts.processes);
        let elapsed = start.elapsed().as_millis() as usize;
        // 성능 40ms 마다, 프로세스 작은 칸 240ms 마다. 느린 CI 서버는 주기가 밀려 덜 모일 수 있어
        // 아래는 "모였다"만, 위는 실제로 흐른 시간으로 엄격히 본다.
        assert!(
            (1..=elapsed / 40 + 4).contains(&performance),
            "성능 {performance}, {elapsed}ms"
        );
        assert!(
            (1..=elapsed / 240 + 2).contains(&processes),
            "프로세스 {processes}, {elapsed}ms"
        );
        assert!(!phone.take().is_empty());

        // 화면이 꺼지면 빈 구독이 온다.
        phone.set(&Subscribe::default());
        // 하던 수집이 끝나기를 기다린 뒤부터 센다.
        thread::sleep(Duration::from_millis(200));
        let stopped = count(&counts.performance);
        thread::sleep(Duration::from_millis(300));
        assert_eq!(count(&counts.performance), stopped);
        assert_eq!(count(&counts.processes), processes);

        // 다시 보면 다시 모으고, 연결이 끝나면 멈춘다.
        phone.set(&watch(View::Processes, &[]));
        wait_for(|| count(&counts.processes) > processes);
        drop(phone);
        thread::sleep(Duration::from_millis(200));
        let gone = count(&counts.processes);
        thread::sleep(Duration::from_millis(300));
        assert_eq!(count(&counts.processes), gone);
        hub.stop();
    });
}

#[test]
fn two_phones_share_one_collection() {
    let counts = Counts::default();
    let hub = hub(&counts);
    thread::scope(|scope| {
        scope.spawn(|| hub.run());
        let _stopper = Stopper(&hub);
        let near = hub.join();
        let far = hub.join();
        near.set(&watch(View::Performance, &[]));
        far.set(&watch(View::Nothing, &[View::Performance]));
        let mut got = (0, 0);
        let start = Instant::now();
        // 고정 시간 대신 여러 번 모일 때까지 본다(느린 CI 서버를 생각해 최대 20초).
        while count(&counts.performance) < 8 && start.elapsed() < Duration::from_secs(20) {
            got.0 += near.take().len();
            got.1 += far.take().len();
            thread::sleep(Duration::from_millis(5));
        }
        thread::sleep(Duration::from_millis(20));
        got.0 += near.take().len();
        got.1 += far.take().len();
        let collected = count(&counts.performance);
        let elapsed = start.elapsed().as_millis() as usize;
        // 수집은 빠른 쪽 주기 한 번뿐이고(두 폰이 따로 모으지 않음), 작은 칸 폰은 그 절반쯤 받는다.
        assert!(
            (8..=elapsed / 40 + 4).contains(&collected),
            "수집 {collected}, {elapsed}ms"
        );
        assert!(got.0 + 2 >= collected, "큰 화면 {got:?}");
        assert!(got.1 >= 2 && got.1 < got.0, "작은 칸 {got:?}");
        hub.stop();
    });
}

#[test]
fn a_command_runs_once_per_number() {
    let counts = Counts::default();
    let hub = hub(&counts);
    let kill = KillProcess {
        request_id: 7,
        pid: 42,
        started_at: 1_000,
    };
    let first = hub.kill(&kill);
    let again = hub.kill(&kill);
    assert!(first.ok);
    assert_eq!(first, again);
    assert_eq!(count(&counts.kills), 1);

    let wrong = hub.kill(&KillProcess {
        request_id: 8,
        pid: 42,
        started_at: 999,
    });
    assert!(!wrong.ok);
    assert_eq!(wrong.reason, "프로세스가 바뀌었습니다");
    assert_eq!(count(&counts.kills), 2);

    let zero = hub.kill(&KillProcess {
        request_id: 9,
        pid: 0,
        started_at: 0,
    });
    assert!(!zero.ok);
    assert_eq!(count(&counts.kills), 2);
}

// 아래는 실제 연결 위에서 폰이 구독하고 명령을 보내는 시험

struct Pipe {
    incoming: Receiver<Vec<u8>>,
    outgoing: Sender<Vec<u8>>,
    /// 초당 바이트. 있으면 보내기를 그만큼 늦춘다(느린 블루투스 흉내).
    rate: Option<usize>,
}

fn pipes() -> (Pipe, Pipe) {
    let (a_tx, a_rx) = mpsc::channel();
    let (b_tx, b_rx) = mpsc::channel();
    (
        Pipe {
            incoming: a_rx,
            outgoing: b_tx,
            rate: None,
        },
        Pipe {
            incoming: b_rx,
            outgoing: a_tx,
            rate: None,
        },
    )
}

impl Conn for Pipe {
    fn receive(&mut self, buffer: &mut [u8]) -> Result<Received, PortError> {
        match self.incoming.recv_timeout(Duration::from_millis(5)) {
            Ok(bytes) => {
                buffer[..bytes.len()].copy_from_slice(&bytes);
                Ok(Received::Bytes(bytes.len()))
            }
            Err(RecvTimeoutError::Timeout) => Ok(Received::Nothing),
            Err(RecvTimeoutError::Disconnected) => Ok(Received::Closed),
        }
    }

    fn send(&mut self, bytes: &[u8]) -> Result<(), PortError> {
        if !bytes.is_empty() {
            if let Some(rate) = self.rate {
                thread::sleep(Duration::from_secs_f64(bytes.len() as f64 / rate as f64));
            }
            let _ = self.outgoing.send(bytes.to_vec());
        }
        Ok(())
    }
}

struct Computer {
    identity: Identity,
    phone: [u8; 32],
    /// 켜지면 등록을 해제한 것처럼 빈 목록을 준다.
    forgotten: AtomicBool,
}

impl ComputerName for Computer {
    fn read(&self) -> Result<String, PortError> {
        Ok("맥".to_owned())
    }
}

impl Keys for Computer {
    fn identity(&self) -> Result<Identity, PortError> {
        Ok(self.identity.clone())
    }

    fn trusted(&self) -> Result<Vec<Paired>, PortError> {
        if self.forgotten.load(Ordering::SeqCst) {
            return Ok(Vec::new());
        }
        Ok(vec![Paired {
            key: self.phone,
            name: "폰".into(),
            serial: "p".into(),
        }])
    }

    fn trust(&self, _: [u8; 32], _: &str, _: &str) -> Result<(), PortError> {
        Ok(())
    }

    fn update(&self, _: [u8; 32], _: &str, _: &str) -> Result<(), PortError> {
        Ok(())
    }

    fn forget(&self, _: [u8; 32]) -> Result<(), PortError> {
        Ok(())
    }
}

impl Prompt for Computer {
    fn ask(&self, _: &str, _: &str) -> Receiver<bool> {
        mpsc::channel().1
    }
}

impl Report for Computer {
    fn note(&self, _: &str, _: sidedesk_domain::Note) {}
}

/// 폰 세션을 기다릴 것이 다 올 때까지 돌린다. Ready 가 되면 구독하고 같은 종료 명령을 두 번 보낸다.
/// `forget` 이면 명령을 보내기 직전에 컴퓨터에서 등록을 해제한다.
fn talk(forget: bool) -> (Result<Ended, ServeError>, Vec<Body>, Counts) {
    talk_with(forget, None)
}

fn talk_with(
    forget: bool,
    deck: Option<Arc<sidedesk_domain::Deck>>,
) -> (Result<Ended, ServeError>, Vec<Body>, Counts) {
    talk_full(forget, deck, None)
}

/// `deck` 이 있으면 종료 대신 단축키 1번 실행, `music` 이 있으면 재생/멈춤 명령을 보낸다.
fn talk_full(
    forget: bool,
    deck: Option<Arc<sidedesk_domain::Deck>>,
    music: Option<Arc<sidedesk_domain::Jukebox>>,
) -> (Result<Ended, ServeError>, Vec<Body>, Counts) {
    talk_over(forget, deck, music, None, Duration::from_millis(400))
}

/// `ble_rate` 가 있으면 블루투스 기기로, 컴퓨터 쪽 보내기를 그 속도로 늦춘다.
fn talk_over(
    forget: bool,
    deck: Option<Arc<sidedesk_domain::Deck>>,
    music: Option<Arc<sidedesk_domain::Jukebox>>,
    ble_rate: Option<usize>,
    window: Duration,
) -> (Result<Ended, ServeError>, Vec<Body>, Counts) {
    let counts = Counts::default();
    let book = deck.as_ref().map(|_| 77);
    let playing = music.is_some();
    let mut hub = match deck {
        Some(deck) => hub(&counts).with_shortcuts(deck),
        None => hub(&counts),
    };
    if let Some(music) = music.clone() {
        hub = hub.with_music(music);
    }
    let phone_identity = Identity::generate();
    let computer = Computer {
        identity: Identity::generate(),
        phone: phone_identity.public,
        forgotten: AtomicBool::new(false),
    };
    let computer_key = computer.identity.public;
    let requests = PairRequests::default();
    let active = Active::default();
    let context = Context {
        name: &computer,
        keys: &computer,
        prompt: &computer,
        report: &computer,
        requests: &requests,
        active: &active,
        hub: &hub,
        timing: Timing {
            ping: Duration::from_secs(1),
            silence: Duration::from_secs(2),
            answer: Duration::from_secs(1),
        },
    };
    let stop = AtomicBool::new(false);
    let (mut mine, mut theirs) = pipes();
    mine.rate = ble_rate;
    let phone = match ble_rate {
        Some(_) => Phone {
            id: "ble:1".into(),
            model: None,
            blocked: None,
            link: sidedesk_domain::Link::Ble,
            beacon: None,
        },
        None => Phone::usb("p", None, None),
    };
    let mut got = Vec::new();
    let ended = thread::scope(|scope| {
        scope.spawn(|| hub.run());
        scope.spawn(|| hub.run_music());
        let _stopper = Stopper(&hub);
        let served = scope.spawn(|| serve(&phone, &mut mine, &context, false, &stop));
        let mut session = Session::new(Role::Phone, &phone_identity, vec![computer_key], false);
        let mut buffer = vec![0; 64 * 1024];
        let start = Instant::now();
        let mut sent = false;
        // 기다릴 것이 다 오거나 연결이 끝날 때까지 본다. `window` 는 최소 한도이고,
        // 느린 CI 서버를 생각해 적어도 20초까지 기다린다.
        let deadline = window.max(Duration::from_secs(20));
        let enough = |got: &[Body]| {
            let count = |pick: fn(&Body) -> bool| got.iter().filter(|body| pick(body)).count();
            let results = count(|body| matches!(body, Body::CommandResult(_))) >= 2;
            if playing {
                // 블루투스면 그림과 그 그림을 가리키는 곡 정보까지
                results
                    && (ble_rate.is_none()
                        || (count(|body| matches!(body, Body::Artwork(_))) >= 1
                            && count(|body| matches!(body, Body::NowPlaying(n) if n.artwork != 0))
                                >= 1))
            } else {
                results
                    && count(|body| matches!(body, Body::Performance(_))) >= 3
                    && (book.is_some() || count(|body| matches!(body, Body::Processes(_))) >= 1)
            }
        };
        while start.elapsed() < deadline && !served.is_finished() && !enough(&got) {
            if let Received::Bytes(n) = theirs.receive(&mut buffer).unwrap() {
                let _ = session.feed(&buffer[..n]);
            }
            while let Some(event) = session.next_event() {
                match event {
                    Event::Ready { .. } if !sent => {
                        sent = true;
                        let view = if playing {
                            watch(View::Music, &[])
                        } else {
                            watch(View::Performance, &[View::Processes])
                        };
                        session.send(Body::Subscribe(view)).unwrap();
                        computer.forgotten.store(forget, Ordering::SeqCst);
                        let kill = KillProcess {
                            request_id: 5,
                            pid: 42,
                            started_at: 1_000,
                        };
                        if playing {
                            let command = sidedesk_core::MusicCommand {
                                request_id: 5,
                                action: sidedesk_core::MusicAction::PlayPause as i32,
                                level: 0,
                            };
                            session.send(Body::MusicCommand(command)).unwrap();
                            session.send(Body::MusicCommand(command)).unwrap();
                        } else if let Some(book) = book {
                            let run = sidedesk_core::RunShortcut {
                                request_id: 5,
                                shortcut_id: 1,
                                book,
                            };
                            session.send(Body::RunShortcut(run.clone())).unwrap();
                            session.send(Body::RunShortcut(run)).unwrap();
                        } else {
                            // 끊긴 줄 알고 다시 보낸 명령
                            session.send(Body::KillProcess(kill.clone())).unwrap();
                            session.send(Body::KillProcess(kill)).unwrap();
                        }
                    }
                    Event::Message(body) => got.push(body),
                    _ => {}
                }
            }
            let _ = theirs.send(&session.take_output());
        }
        stop.store(true, Ordering::Relaxed);
        served.join().unwrap()
    });
    (ended, got, counts)
}

#[test]
fn a_phone_subscribes_and_kills_over_the_link() {
    let (ended, got, counts) = talk(false);
    assert_eq!(ended, Ok(Ended::Stopped));
    let results: Vec<_> = got
        .iter()
        .filter_map(|body| match body {
            Body::CommandResult(result) => Some(result.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(results.len(), 2);
    assert!(
        results
            .iter()
            .all(|result| result.ok && result.request_id == 5)
    );
    assert_eq!(count(&counts.kills), 1);
    let performance = got
        .iter()
        .filter(|body| matches!(body, Body::Performance(_)))
        .count();
    assert!(performance >= 3, "성능 {performance}");
    assert!(
        got.iter()
            .any(|body| matches!(body, Body::Processes(p) if p.items.len() == 1))
    );
}

#[test]
fn a_forgotten_phone_cannot_kill() {
    let (ended, got, counts) = talk(true);
    assert_eq!(ended, Err(ServeError::Forgotten));
    assert_eq!(count(&counts.kills), 0);
    assert!(
        !got.iter()
            .any(|body| matches!(body, Body::CommandResult(_)))
    );
}

/// 처음 한 번 panic 하는 수집기
struct Shaky(AtomicUsize);

impl Meter for Shaky {
    fn performance(&self) -> Result<Performance, PortError> {
        if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
            panic!("수집기 시험용 panic");
        }
        Ok(Performance::default())
    }
}

#[test]
fn a_panicking_collector_is_tried_again() {
    let counts = Counts::default();
    let shaky = Shaky(AtomicUsize::new(0));
    let hub = Hub::new(Box::new(shaky), Box::new(Fake(counts)), cadence());
    thread::scope(|scope| {
        let runner = scope.spawn(|| hub.run());
        let _stopper = Stopper(&hub);
        let phone = hub.join();
        phone.set(&watch(View::Performance, &[]));
        let start = Instant::now();
        let mut got = 0;
        while start.elapsed() < Duration::from_secs(20) && got == 0 {
            got += phone.take().len();
            thread::sleep(Duration::from_millis(5));
        }
        assert!(got > 0);
        assert!(!runner.is_finished());
        hub.stop();
    });
}

/// 처음 한 번 panic 하고, 그 뒤로는 부른 횟수를 센다.
struct Sky(AtomicUsize);

impl sidedesk_domain::WeatherSource for Sky {
    fn fetch(&self, _place: sidedesk_domain::Place) -> Result<sidedesk_core::Weather, PortError> {
        if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
            panic!("날씨 시험용 panic");
        }
        Ok(sidedesk_core::Weather {
            temperature: 7.0,
            ..Default::default()
        })
    }
}

fn quick_weather() -> sidedesk_domain::WeatherCadence {
    let ms = Duration::from_millis;
    sidedesk_domain::WeatherCadence {
        every: ms(150),
        retry: [ms(20), ms(20), ms(20), ms(20), ms(20)],
        nap: ms(30),
        keep: ms(200),
    }
}

#[test]
fn phones_at_one_place_share_weather_and_stop_ends_the_thread() {
    let calls = Arc::new(Sky(AtomicUsize::new(0)));
    struct Shared(Arc<Sky>);
    impl sidedesk_domain::WeatherSource for Shared {
        fn fetch(
            &self,
            place: sidedesk_domain::Place,
        ) -> Result<sidedesk_core::Weather, PortError> {
            self.0.fetch(place)
        }
    }
    let counts = Counts::default();
    let hub = hub(&counts).with_weather(Box::new(Shared(Arc::clone(&calls))), quick_weather());
    let seoul = sidedesk_core::Location {
        latitude_tenths: Some(376),
        longitude_tenths: Some(1270),
    };
    thread::scope(|scope| {
        let runner = scope.spawn(|| hub.run_weather());
        let _stopper = Stopper(&hub);
        let a = hub.join();
        let b = hub.join();
        for phone in [&a, &b] {
            phone.place(&seoul);
            phone.set(&watch(View::Weather, &[]));
        }
        let mut got = (0, 0);
        let start = Instant::now();
        // 두 기기가 하나씩 받고 실패 뒤 다시 받기까지 될 때까지(최대 20초), 그 뒤 주기 하나를 더 본다.
        let mut settled: Option<Instant> = None;
        while start.elapsed() < Duration::from_secs(20)
            && settled.is_none_or(|at| at.elapsed() < Duration::from_millis(200))
        {
            if settled.is_none() && got.0 >= 1 && got.1 >= 1 {
                settled = Some(Instant::now());
            }
            for (phone, n) in [(&a, &mut got.0), (&b, &mut got.1)] {
                for body in phone.take() {
                    if let Body::Weather(w) = body {
                        assert_eq!((w.latitude_tenths, w.longitude_tenths), (376, 1270));
                        assert!(w.fetched_at > 0);
                        *n += 1;
                    }
                }
            }
            thread::sleep(Duration::from_millis(5));
        }
        // 첫 번째는 panic(실패), 20ms 뒤 성공, 그 뒤 150ms 마다. 두 기기가 함께 쓴다(기기마다 받지 않음).
        let fetched = calls.0.load(Ordering::SeqCst);
        let elapsed = start.elapsed().as_millis() as usize;
        assert!(
            (2..=elapsed / 150 + 4).contains(&fetched),
            "받기 {fetched}, {elapsed}ms"
        );
        assert!(got.0 >= 1 && got.1 >= 1, "{got:?}");
        // 성능은 아무도 안 봐서 모으지 않았다.
        assert_eq!(count(&counts.performance), 0);

        // 둘 다 날씨를 안 보면 더 받지 않는다.
        for phone in [&a, &b] {
            phone.set(&Subscribe::default());
        }
        // 하던 받기가 끝나기를 기다린 뒤부터 센다.
        thread::sleep(Duration::from_millis(300));
        let stopped = calls.0.load(Ordering::SeqCst);
        thread::sleep(Duration::from_millis(300));
        assert_eq!(calls.0.load(Ordering::SeqCst), stopped);

        hub.stop();
        let start = Instant::now();
        while !runner.is_finished() {
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "날씨 스레드가 끝나지 않음"
            );
            thread::sleep(Duration::from_millis(5));
        }
    });
}

#[test]
fn without_a_weather_source_the_thread_ends_at_once() {
    let counts = Counts::default();
    let hub = hub(&counts);
    hub.run_weather();
}

#[test]
fn weather_notices_do_not_disturb_performance_collection() {
    let calls = Arc::new(Sky(AtomicUsize::new(1)));
    struct Shared(Arc<Sky>);
    impl sidedesk_domain::WeatherSource for Shared {
        fn fetch(
            &self,
            place: sidedesk_domain::Place,
        ) -> Result<sidedesk_core::Weather, PortError> {
            self.0.fetch(place)
        }
    }
    let counts = Counts::default();
    let hub = hub(&counts).with_weather(Box::new(Shared(calls)), quick_weather());
    thread::scope(|scope| {
        scope.spawn(|| hub.run());
        scope.spawn(|| hub.run_weather());
        let _stopper = Stopper(&hub);
        let phone = hub.join();
        phone.set(&watch(View::Performance, &[View::Weather]));
        // 위치를 계속 바꿔 날씨 쪽만 깨운다.
        let start = Instant::now();
        let mut tenths = 0;
        while start.elapsed() < Duration::from_millis(400) {
            tenths = (tenths + 1) % 50;
            phone.place(&sidedesk_core::Location {
                latitude_tenths: Some(370 + tenths),
                longitude_tenths: Some(1270),
            });
            thread::sleep(Duration::from_millis(1));
        }
        // 성능은 40ms 주기 그대로. 날씨 알림마다 더 모으지 않는다.
        // 느린 CI 서버에서는 주기가 밀려 덜 모일 수 있으므로, 실제로 흐른 시간으로 위쪽 한도만 엄격히 본다.
        let performance = count(&counts.performance);
        let ticks = start.elapsed().as_millis() as usize / 40;
        assert!(
            (1..=ticks + 4).contains(&performance),
            "성능 {performance}, 주기 {ticks}"
        );
        hub.stop();
    });
}

/// 시험이 단계를 바꿔 주는 가짜 기록
struct Log(Mutex<Vec<sidedesk_domain::SessionFacts>>, AtomicUsize);

impl sidedesk_domain::SessionLog for Log {
    fn poll(&self) -> Result<Vec<sidedesk_domain::SessionFacts>, PortError> {
        self.1.fetch_add(1, Ordering::SeqCst);
        Ok(self.0.lock().unwrap().clone())
    }
}

struct SharedLog(Arc<Log>);

impl sidedesk_domain::SessionLog for SharedLog {
    fn poll(&self) -> Result<Vec<sidedesk_domain::SessionFacts>, PortError> {
        self.0.poll()
    }
}

/// 숫자를 주고, 끝낼 때 불렸는지 남기는 가짜 gh/glab
struct Counter(AtomicUsize, AtomicBool);

impl sidedesk_domain::Forges for Counter {
    fn counts(&self) -> sidedesk_core::Forge {
        self.0.fetch_add(1, Ordering::SeqCst);
        sidedesk_core::Forge::default()
    }

    fn stop(&self) {
        self.1.store(true, Ordering::SeqCst);
    }
}

struct SharedForge(Arc<Counter>);

impl sidedesk_domain::Forges for SharedForge {
    fn counts(&self) -> sidedesk_core::Forge {
        self.0.counts()
    }

    fn stop(&self) {
        self.0.stop()
    }
}

fn session(phase: sidedesk_domain::Phase, seconds_ago: u64) -> sidedesk_domain::SessionFacts {
    sidedesk_domain::SessionFacts {
        key: "claude:1".into(),
        tool: sidedesk_core::AiTool::ClaudeCode,
        title: "작업".into(),
        project: "폴더".into(),
        open: true,
        phase,
        last_activity: std::time::SystemTime::now() - Duration::from_secs(seconds_ago),
    }
}

fn quick_ai() -> sidedesk_domain::AiCadence {
    sidedesk_domain::AiCadence {
        poll: Duration::from_millis(20),
        ..sidedesk_domain::AiCadence::default()
    }
}

/// 느린 CI 서버를 생각해 고정 시간 대신 조건이 될 때까지 기다린다(최대 20초).
fn wait_for(mut done: impl FnMut() -> bool) {
    let start = Instant::now();
    while !done() {
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "기다려도 되지 않음"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn ai_sessions_flow_and_stop_quietly_when_nobody_watches() {
    use sidedesk_domain::Phase;
    let log = Arc::new(Log(
        Mutex::new(vec![session(Phase::Working, 0)]),
        AtomicUsize::new(0),
    ));
    let forge = Arc::new(Counter(AtomicUsize::new(0), AtomicBool::new(false)));
    let counts = Counts::default();
    let hub = hub(&counts).with_ai(
        Box::new(SharedLog(Arc::clone(&log))),
        Some(Box::new(SharedForge(Arc::clone(&forge)))),
        quick_ai(),
    );
    thread::scope(|scope| {
        scope.spawn(|| hub.run_ai());
        scope.spawn(|| hub.run_forge());
        let _stopper = Stopper(&hub);
        // 아무도 안 보면 읽지 않는다.
        thread::sleep(Duration::from_millis(60));
        assert_eq!(log.1.load(Ordering::SeqCst), 0);

        let near = hub.join();
        let far = hub.join();
        near.set(&watch(View::Ai, &[]));
        far.set(&watch(View::Performance, &[View::Ai]));
        let got = |phone: &sidedesk_domain::Subscriber<'_>| {
            phone
                .take()
                .into_iter()
                .fold((0, 0, 0), |(s, a, f), body| match body {
                    Body::AiSessions(_) => (s + 1, a, f),
                    Body::AiAlert(_) => (s, a + 1, f),
                    Body::Forge(_) => (s, a, f + 1),
                    _ => (s, a, f),
                })
        };
        wait_for(|| log.1.load(Ordering::SeqCst) >= 2);
        let (sessions, alerts, forges) = got(&near);
        assert_eq!((sessions, alerts), (1, 0));
        assert_eq!(forges, 1);
        assert_eq!(got(&far), (1, 0, 0));
        assert_eq!(forge.0.load(Ordering::SeqCst), 1);

        // 답변 대기로 바뀌면 두 기기 모두 알림 하나
        *log.0.lock().unwrap() = vec![session(Phase::Waiting, 0)];
        let mut near_got = (0, 0, 0);
        let mut far_got = (0, 0, 0);
        wait_for(|| {
            let a = got(&near);
            let b = got(&far);
            near_got = (near_got.0 + a.0, near_got.1 + a.1, 0);
            far_got = (far_got.0 + b.0, far_got.1 + b.1, 0);
            near_got.1 == 1 && far_got.1 == 1
        });

        // 둘 다 안 보는 사이 끝났다가 다시 보면 알림이 없다.
        *log.0.lock().unwrap() = vec![session(Phase::Working, 0)];
        // 작업 중을 적어도 한 번 읽은 뒤에 둘 다 안 보게 한다.
        let seen = log.1.load(Ordering::SeqCst);
        wait_for(|| log.1.load(Ordering::SeqCst) >= seen + 2);
        near.set(&Subscribe::default());
        far.set(&Subscribe::default());
        thread::sleep(Duration::from_millis(200));
        let polled = log.1.load(Ordering::SeqCst);
        *log.0.lock().unwrap() = vec![session(Phase::TurnEnded, 60)];
        thread::sleep(Duration::from_millis(100));
        assert_eq!(log.1.load(Ordering::SeqCst), polled);
        near.set(&watch(View::Ai, &[]));
        let (mut sessions, mut alerts) = (0, 0);
        wait_for(|| {
            let (s, a, _) = got(&near);
            sessions += s;
            alerts += a;
            sessions >= 1
        });
        // 늦게 오는 알림이 없는지 잠깐 더 본다.
        thread::sleep(Duration::from_millis(100));
        alerts += got(&near).1;
        assert_eq!(alerts, 0);

        hub.stop();
    });
    assert!(forge.1.load(Ordering::SeqCst));
}

/// 단축키 하나(주소 열기)를 든 장부와 부른 횟수
fn url_deck() -> (Arc<sidedesk_domain::Deck>, Arc<AtomicUsize>) {
    struct Store;
    impl sidedesk_domain::ShortcutStore for Store {
        fn load(&self) -> Result<(sidedesk_domain::Saved, Option<String>), PortError> {
            let mut saved = sidedesk_domain::Saved::new(77);
            saved.items.push(sidedesk_domain::Shortcut {
                id: 1,
                name: "열기".into(),
                icon: "browser".into(),
                color: "orange".into(),
                action: sidedesk_domain::Action::Url {
                    url: "https://example.com".into(),
                },
            });
            saved.next_id = 2;
            Ok((saved, None))
        }
        fn save(&self, _: &sidedesk_domain::Saved) -> Result<(), PortError> {
            Ok(())
        }
    }
    struct Runs(Arc<AtomicUsize>);
    impl sidedesk_domain::Launcher for Runs {
        fn run(&self, _: &sidedesk_domain::Action) -> Result<(), PortError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        fn toggle(&self, _: &sidedesk_domain::Action) -> Option<bool> {
            None
        }
        fn can_send_keys(&self) -> bool {
            true
        }
    }
    let runs = Arc::new(AtomicUsize::new(0));
    (
        sidedesk_domain::Deck::new(Box::new(Store), Arc::new(Runs(Arc::clone(&runs)))),
        runs,
    )
}

#[test]
fn a_shortcut_runs_once_over_the_link() {
    let (deck, runs) = url_deck();
    let (ended, got, _) = talk_with(false, Some(deck));
    assert_eq!(ended, Ok(Ended::Stopped));
    let results: Vec<_> = got
        .iter()
        .filter_map(|body| match body {
            Body::CommandResult(result) => Some(result.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(results.len(), 2, "{got:?}");
    assert!(results.iter().all(|r| r.ok && r.request_id == 5));
    assert_eq!(runs.load(Ordering::SeqCst), 1);
}

#[test]
fn a_forgotten_phone_cannot_run_shortcuts() {
    let (deck, runs) = url_deck();
    let (ended, got, _) = talk_with(true, Some(deck));
    assert_eq!(ended, Err(ServeError::Forgotten));
    assert_eq!(runs.load(Ordering::SeqCst), 0);
    assert!(
        !got.iter()
            .any(|body| matches!(body, Body::CommandResult(_)))
    );
}

#[test]
fn shortcut_subscriptions_and_a_hub_without_a_deck() {
    // 장부가 없으면 "없는 단축키"로 바로 거절한다.
    let counts = Counts::default();
    let bare = hub(&counts);
    let phone = bare.join();
    phone.run_shortcut(&sidedesk_core::RunShortcut {
        request_id: 1,
        shortcut_id: 1,
        book: 1,
    });
    let results: Vec<_> = phone
        .take()
        .into_iter()
        .filter_map(|body| match body {
            Body::CommandResult(result) => Some(result),
            _ => None,
        })
        .collect();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].reason, "없는 단축키입니다");
    drop(phone);

    // tiles 에 단축키가 있으면 목록이 오고, 빼면 더 오지 않는다.
    let (deck, _) = url_deck();
    let hub = hub(&counts).with_shortcuts(deck);
    let phone = hub.join();
    phone.set(&watch(View::Performance, &[View::Shortcuts]));
    let lists = |phone: &sidedesk_domain::Subscriber<'_>| {
        phone
            .take()
            .into_iter()
            .filter(|body| matches!(body, Body::Shortcuts(s) if s.book == 77 && s.items.len() == 1))
            .count()
    };
    assert_eq!(lists(&phone), 1);
    phone.set(&watch(View::Performance, &[]));
    phone.set(&watch(View::Shortcuts, &[]));
    assert_eq!(lists(&phone), 1);
}

/// 재생/멈춤 수를 세는 가짜 플레이어
struct Deejay(Arc<AtomicUsize>);

impl sidedesk_domain::Player for Deejay {
    fn now(&self) -> Result<(sidedesk_domain::Track, Option<sidedesk_domain::Picture>), PortError> {
        Ok((sidedesk_domain::Track::nothing(), None))
    }
    fn command(&self, _: sidedesk_core::MusicAction) -> Result<(), PortError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn watch(&self, _: Option<sidedesk_domain::Changed>) {}
}

struct Mute;

impl sidedesk_domain::Speaker for Mute {
    fn read(&self) -> Option<sidedesk_domain::Level> {
        None
    }
    fn set(&self, _: u32) -> Result<(), PortError> {
        Err(PortError::new("볼륨을 바꿀 수 없는 장치입니다"))
    }
}

fn jukebox() -> (Arc<sidedesk_domain::Jukebox>, Arc<AtomicUsize>) {
    let plays = Arc::new(AtomicUsize::new(0));
    (
        Arc::new(sidedesk_domain::Jukebox::new(
            Box::new(Deejay(Arc::clone(&plays))),
            Box::new(Mute),
            sidedesk_domain::MusicCadence::default(),
        )),
        plays,
    )
}

#[test]
fn a_music_command_runs_once_over_the_link() {
    let (music, plays) = jukebox();
    let (ended, got, _) = talk_full(false, None, Some(music));
    assert_eq!(ended, Ok(Ended::Stopped));
    let results = got
        .iter()
        .filter(|body| matches!(body, Body::CommandResult(r) if r.ok && r.request_id == 5))
        .count();
    assert_eq!(results, 2, "{got:?}");
    assert_eq!(plays.load(Ordering::SeqCst), 1);
}

#[test]
fn a_forgotten_phone_cannot_control_music() {
    let (music, plays) = jukebox();
    let (ended, _, _) = talk_full(true, None, Some(music));
    assert_eq!(ended, Err(ServeError::Forgotten));
    assert_eq!(plays.load(Ordering::SeqCst), 0);
}

/// 큰 그림(40KB)과 블루투스용 작은 판(20KB)을 주는 가짜 플레이어
struct Painter;

impl sidedesk_domain::Player for Painter {
    fn now(&self) -> Result<(sidedesk_domain::Track, Option<sidedesk_domain::Picture>), PortError> {
        let mut track = sidedesk_domain::Track::nothing();
        track.player = sidedesk_core::MusicPlayer::Music;
        track.key = "곡".into();
        let small = sidedesk_domain::Picture {
            hash: 2,
            jpeg: vec![0xAB; 20 * 1024],
            small: None,
        };
        Ok((
            track,
            Some(sidedesk_domain::Picture {
                hash: 1,
                jpeg: vec![0xCD; 40 * 1024],
                small: Some(Box::new(small)),
            }),
        ))
    }
    fn command(&self, _: sidedesk_core::MusicAction) -> Result<(), PortError> {
        Ok(())
    }
    fn watch(&self, _: Option<sidedesk_domain::Changed>) {}
}

#[test]
fn a_slow_bluetooth_link_gets_the_small_picture_and_stays_up() {
    let music = Arc::new(sidedesk_domain::Jukebox::new(
        Box::new(Painter),
        Box::new(Mute),
        sidedesk_domain::MusicCadence::default(),
    ));
    // 초당 8KB 로 늦춘 블루투스: 20KB 그림이 약 2.5초 걸린다.
    let (ended, got, _) = talk_over(
        false,
        None,
        Some(music),
        Some(8 * 1024),
        Duration::from_secs(5),
    );
    assert_eq!(ended, Ok(Ended::Stopped), "{got:?}");
    let art: Vec<_> = got
        .iter()
        .filter_map(|body| match body {
            Body::Artwork(a) => Some(a),
            _ => None,
        })
        .collect();
    assert_eq!(art.len(), 1);
    assert_eq!(art[0].hash, 2);
    assert!(art[0].jpeg.len() <= sidedesk_domain::BLE_ART_LIMIT);
    assert!(
        got.iter()
            .any(|b| matches!(b, Body::NowPlaying(n) if n.artwork == 2))
    );
}
