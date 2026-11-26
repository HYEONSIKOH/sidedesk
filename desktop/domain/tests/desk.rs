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
    Active, Cadence, ComputerName, Conn, Context, Hub, Keys, Meter, PairRequests, Paired, Phone,
    PortError, ProcessTable, Prompt, Received, Report, Timing, serve,
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
        assert!((7..=12).contains(&performance), "성능 {performance}");
        assert!((1..=3).contains(&processes), "프로세스 {processes}");
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
        assert!((10..=14).contains(&collected), "수집 {collected}");
        assert!(got.0 >= collected - 1, "큰 화면 {got:?}");
        assert!((5..=8).contains(&got.1), "작은 칸 {got:?}");
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

#[test]
fn a_phone_subscribes_and_kills_over_the_link() {
    let counts = Counts::default();
    let hub = hub(&counts);
    let phone_identity = Identity::generate();
    let computer = Computer {
        identity: Identity::generate(),
        phone: phone_identity.public,
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
            ping: Duration::from_millis(50),
            silence: Duration::from_secs(2),
            answer: Duration::from_secs(1),
        },
    };
    let stop = AtomicBool::new(false);
    let (mut mine, mut theirs) = pipes();
    let got: Mutex<Vec<Body>> = Mutex::default();
    thread::scope(|scope| {
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
                session.feed(&buffer[..n]).unwrap();
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
                        let kill = KillProcess {
                            request_id: 5,
                            pid: 42,
                            started_at: 1_000,
                        };
                        // 끊긴 줄 알고 다시 보낸 명령
                        session.send(Body::KillProcess(kill.clone())).unwrap();
                        session.send(Body::KillProcess(kill)).unwrap();
                    }
                    Event::Message(body) => got.lock().unwrap().push(body),
                    _ => {}
                }
            }
            theirs.send(&session.take_output()).unwrap();
        }
        stop.store(true, Ordering::Relaxed);
        served.join().unwrap().unwrap();
        hub.stop();
    });
    let got = got.into_inner().unwrap();
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
    assert!(performance >= 5, "성능 {performance}");
    assert!(
        got.iter()
            .any(|body| matches!(body, Body::Processes(p) if p.items.len() == 1))
    );
}
