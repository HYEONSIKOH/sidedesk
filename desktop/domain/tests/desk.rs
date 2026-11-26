//! 구독, 수집 주기, 멈춤, 명령 중복을 가짜 수집기와 가짜 폰으로 시험한다.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
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
        thread::sleep(Duration::from_millis(400));
        let performance = count(&counts.performance);
        let processes = count(&counts.processes);
        // 400ms 동안 성능 40ms 마다(약 10번), 프로세스 작은 칸 240ms 마다(약 2번)
        assert!((5..=14).contains(&performance), "성능 {performance}");
        assert!((1..=4).contains(&processes), "프로세스 {processes}");
        assert!(!phone.take().is_empty());

        // 화면이 꺼지면 빈 구독이 온다.
        phone.set(&Subscribe::default());
        thread::sleep(Duration::from_millis(50));
        let stopped = count(&counts.performance);
        thread::sleep(Duration::from_millis(300));
        assert_eq!(count(&counts.performance), stopped);
        assert_eq!(count(&counts.processes), processes);

        // 다시 보면 다시 모으고, 연결이 끝나면 멈춘다.
        phone.set(&watch(View::Processes, &[]));
        thread::sleep(Duration::from_millis(200));
        assert!(count(&counts.processes) > processes);
        drop(phone);
        thread::sleep(Duration::from_millis(50));
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
        while start.elapsed() < Duration::from_millis(500) {
            got.0 += near.take().len();
            got.1 += far.take().len();
            thread::sleep(Duration::from_millis(5));
        }
        let collected = count(&counts.performance);
        // 수집은 빠른 쪽 주기 한 번뿐이고, 작은 칸 폰은 그 절반쯤 받는다.
        assert!((6..=16).contains(&collected), "수집 {collected}");
        assert!(got.0 + 2 >= collected, "큰 화면 {got:?}");
        assert!((3..=9).contains(&got.1) && got.1 < got.0, "작은 칸 {got:?}");
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
}

fn pipes() -> (Pipe, Pipe) {
    let (a_tx, a_rx) = mpsc::channel();
    let (b_tx, b_rx) = mpsc::channel();
    (
        Pipe {
            incoming: a_rx,
            outgoing: b_tx,
        },
        Pipe {
            incoming: b_rx,
            outgoing: a_tx,
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

/// 폰 세션을 400ms 돌린다. Ready 가 되면 구독하고 같은 종료 명령을 두 번 보낸다.
/// `forget` 이면 명령을 보내기 직전에 컴퓨터에서 등록을 해제한다.
fn talk(forget: bool) -> (Result<Ended, ServeError>, Vec<Body>, Counts) {
    let counts = Counts::default();
    let hub = hub(&counts);
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
    let mut got = Vec::new();
    let ended = thread::scope(|scope| {
        scope.spawn(|| hub.run());
        let _stopper = Stopper(&hub);
        let served = scope.spawn(|| {
            serve(
                &Phone::usb("p", None, None),
                &mut mine,
                &context,
                false,
                &stop,
            )
        });
        let mut session = Session::new(Role::Phone, &phone_identity, vec![computer_key], false);
        let mut buffer = vec![0; 64 * 1024];
        let start = Instant::now();
        let mut sent = false;
        while start.elapsed() < Duration::from_millis(400) {
            if let Received::Bytes(n) = theirs.receive(&mut buffer).unwrap() {
                let _ = session.feed(&buffer[..n]);
            }
            while let Some(event) = session.next_event() {
                match event {
                    Event::Ready { .. } if !sent => {
                        sent = true;
                        session
                            .send(Body::Subscribe(watch(
                                View::Performance,
                                &[View::Processes],
                            )))
                            .unwrap();
                        computer.forgotten.store(forget, Ordering::SeqCst);
                        let kill = KillProcess {
                            request_id: 5,
                            pid: 42,
                            started_at: 1_000,
                        };
                        // 끊긴 줄 알고 다시 보낸 명령
                        session.send(Body::KillProcess(kill.clone())).unwrap();
                        session.send(Body::KillProcess(kill)).unwrap();
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
        while start.elapsed() < Duration::from_millis(300) && got == 0 {
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
        while start.elapsed() < Duration::from_millis(250) {
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
        // 첫 번째는 panic(실패), 20ms 뒤 성공, 150ms 뒤 한 번 더. 두 기기가 함께 쓴다.
        let fetched = calls.0.load(Ordering::SeqCst);
        assert!((2..=4).contains(&fetched), "받기 {fetched}");
        assert!(got.0 >= 1 && got.1 >= 1, "{got:?}");
        // 성능은 아무도 안 봐서 모으지 않았다.
        assert_eq!(count(&counts.performance), 0);

        // 둘 다 날씨를 안 보면 더 받지 않는다.
        for phone in [&a, &b] {
            phone.set(&Subscribe::default());
        }
        thread::sleep(Duration::from_millis(30));
        let stopped = calls.0.load(Ordering::SeqCst);
        thread::sleep(Duration::from_millis(300));
        assert_eq!(calls.0.load(Ordering::SeqCst), stopped);

        hub.stop();
        let start = Instant::now();
        while !runner.is_finished() {
            assert!(
                start.elapsed() < Duration::from_secs(1),
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
