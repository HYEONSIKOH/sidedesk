//! 실제 폰 대신 공용 코어의 폰 쪽 세션을 스레드에서 돌려 컴퓨터 쪽을 시험한다.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use sidedesk_core::{Body, Event, Identity, KEY_LEN, Role, Session, beacon};
use sidedesk_core::{Performance, Process};
use sidedesk_domain::{
    Active, Cadence, ComputerName, Conn, Context, Ended, Hub, Keys, Link, Meter, Note,
    PairRequests, Paired, Phone, PhoneLinks, PortError, ProcessTable, Prompt, Received, Report,
    ServeError, Source, Timing, keep_connected, serve,
};

struct Pipe {
    incoming: Receiver<Vec<u8>>,
    outgoing: Sender<Vec<u8>>,
    /// 상대가 닫은 뒤 보내면 오류를 낸다(실제 블루투스처럼).
    strict: bool,
}

fn pipes() -> (Pipe, Pipe) {
    let (a_tx, a_rx) = mpsc::channel();
    let (b_tx, b_rx) = mpsc::channel();
    (
        Pipe {
            incoming: a_rx,
            outgoing: b_tx,
            strict: false,
        },
        Pipe {
            incoming: b_rx,
            outgoing: a_tx,
            strict: false,
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
        if !bytes.is_empty() && self.outgoing.send(bytes.to_vec()).is_err() && self.strict {
            return Err(PortError::new("블루투스 연결이 끊겼습니다"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq)]
enum PhoneSays {
    Yes,
    No,
    /// 첫 메시지를 받으면 그 뒤로 아무것도 보내지 않는다.
    Mute,
    /// 숫자를 보고도 답하지 않는다. Ping 에는 답한다.
    Ignore,
    /// 이름을 받아도 닫지 않고 계속 붙어 있는다.
    Stay,
    /// 연결이 Ready 가 되자마자 닫는다(다른 컴퓨터가 쓰고 있는 기기).
    Busy,
}

/// 폰 쪽 세션. 컴퓨터 이름을 받으면 연결을 닫는다(Stay 는 빼고).
fn phone(
    identity: Identity,
    trusted: Vec<[u8; KEY_LEN]>,
    says: PhoneSays,
    pipe: Pipe,
) -> JoinHandle<Vec<Event>> {
    named_phone(identity, trusted, says, pipe, "")
}

/// 기종 이름을 알리는 폰
fn named_phone(
    identity: Identity,
    trusted: Vec<[u8; KEY_LEN]>,
    says: PhoneSays,
    mut pipe: Pipe,
    name: &'static str,
) -> JoinHandle<Vec<Event>> {
    thread::spawn(move || {
        let mut session = Session::new(Role::Phone, &identity, trusted, false);
        session.set_name(name);
        let mut events = Vec::new();
        let mut buffer = vec![0; 16 * 1024];
        loop {
            match pipe.receive(&mut buffer).unwrap() {
                Received::Bytes(count) => {
                    if session.feed(&buffer[..count]).is_err() {
                        pipe.send(&session.take_output()).unwrap();
                        return events;
                    }
                }
                Received::Nothing => {}
                Received::Closed => return events,
            }
            while let Some(event) = session.next_event() {
                if says == PhoneSays::Busy && matches!(event, Event::Ready { .. }) {
                    let _ = pipe.send(&session.take_output());
                    return events;
                }
                if let Event::PairingCode(_) = event
                    && says != PhoneSays::Ignore
                {
                    let _ = session.confirm(says != PhoneSays::No);
                }
                let done =
                    matches!(event, Event::Message(Body::Hello(_))) && says != PhoneSays::Stay;
                events.push(event);
                if done {
                    return events;
                }
            }
            if says == PhoneSays::Mute {
                let _ = session.take_output();
                continue;
            }
            pipe.send(&session.take_output()).unwrap();
        }
    })
}

fn p() -> Phone {
    Phone::usb("p", Some("SM A716S".into()), None)
}

fn ble(key: &[u8; KEY_LEN]) -> Phone {
    Phone {
        id: "ble:1".into(),
        model: Some("Galaxy A71".into()),
        blocked: None,
        link: Link::Ble,
        beacon: Some(beacon(key)),
    }
}

struct Name;

impl ComputerName for Name {
    fn read(&self) -> Result<String, PortError> {
        Ok("맥".to_owned())
    }
}

struct MemoryKeys {
    identity: Identity,
    trusted: Mutex<Vec<Paired>>,
}

impl MemoryKeys {
    fn keys(&self) -> Vec<[u8; KEY_LEN]> {
        self.trusted
            .lock()
            .unwrap()
            .iter()
            .map(|paired| paired.key)
            .collect()
    }
}

impl Keys for MemoryKeys {
    fn identity(&self) -> Result<Identity, PortError> {
        Ok(self.identity.clone())
    }

    fn trusted(&self) -> Result<Vec<Paired>, PortError> {
        Ok(self.trusted.lock().unwrap().clone())
    }

    fn trust(&self, key: [u8; KEY_LEN], name: &str, serial: &str) -> Result<(), PortError> {
        let mut trusted = self.trusted.lock().unwrap();
        trusted.retain(|paired| paired.key != key);
        trusted.push(Paired {
            key,
            name: name.to_owned(),
            serial: serial.to_owned(),
        });
        Ok(())
    }

    fn update(&self, key: [u8; KEY_LEN], name: &str, serial: &str) -> Result<(), PortError> {
        if let Some(saved) = self
            .trusted
            .lock()
            .unwrap()
            .iter_mut()
            .find(|p| p.key == key)
        {
            saved.name = name.to_owned();
            saved.serial = serial.to_owned();
        }
        Ok(())
    }

    fn forget(&self, key: [u8; KEY_LEN]) -> Result<(), PortError> {
        self.trusted
            .lock()
            .unwrap()
            .retain(|paired| paired.key != key);
        Ok(())
    }
}

/// None 이면 답하지 않는다.
struct Person(Option<bool>, Mutex<Vec<Sender<bool>>>);

impl Prompt for Person {
    fn ask(&self, _phone: &str, code: &str) -> Receiver<bool> {
        assert_eq!(code.len(), 6);
        let (tx, rx) = mpsc::channel();
        match self.0 {
            Some(same) => tx.send(same).unwrap(),
            None => self.1.lock().unwrap().push(tx),
        }
        rx
    }
}

#[derive(Default)]
struct Notes(
    Mutex<Vec<(String, Note)>>,
    Mutex<Vec<(Source, Vec<String>)>>,
);

impl Report for Notes {
    fn note(&self, phone: &str, note: Note) {
        self.0.lock().unwrap().push((phone.to_owned(), note));
    }

    fn phones(&self, source: Source, phones: &[Phone]) {
        let ids = phones.iter().map(|phone| phone.id.clone()).collect();
        self.1.lock().unwrap().push((source, ids));
    }
}

/// 이 시험들은 구독하지 않으므로 수집기는 불리지 않는다.
struct Unused;

impl Meter for Unused {
    fn performance(&self) -> Result<Performance, PortError> {
        unreachable!()
    }
}

impl ProcessTable for Unused {
    fn processes(&self) -> Result<Vec<Process>, PortError> {
        unreachable!()
    }

    fn kill(&self, _pid: u32, _started_at: u64) -> Result<(), PortError> {
        unreachable!()
    }
}

struct World {
    hub: Hub,
    computer: MemoryKeys,
    requests: PairRequests,
    person: Person,
    notes: Notes,
    active: Active,
    timing: Timing,
}

impl World {
    fn new(trusted: Vec<[u8; KEY_LEN]>, answer: Option<bool>) -> Self {
        let trusted = trusted
            .into_iter()
            .map(|key| Paired {
                key,
                name: String::new(),
                serial: String::new(),
            })
            .collect();
        Self {
            hub: Hub::new(Box::new(Unused), Box::new(Unused), Cadence::default()),
            computer: MemoryKeys {
                identity: Identity::generate(),
                trusted: Mutex::new(trusted),
            },
            requests: PairRequests::default(),
            person: Person(answer, Mutex::default()),
            notes: Notes::default(),
            active: Active::default(),
            // 침묵 한계는 넉넉히: 느린 서버에서 시험 스레드가 굶어도 연결이 침묵으로 먼저 끊기지 않게.
            // 답 기다리기(answer)와 핑은 짧게 두어 시험이 빨리 끝난다. 침묵 자체는 a_silent_phone_is_dropped 가 본다.
            timing: Timing {
                ping: Duration::from_millis(20),
                silence: Duration::from_secs(3),
                answer: Duration::from_millis(150),
            },
        }
    }

    fn context(&self) -> Context<'_> {
        Context {
            name: &Name,
            keys: &self.computer,
            prompt: &self.person,
            report: &self.notes,
            requests: &self.requests,
            active: &self.active,
            hub: &self.hub,
            timing: self.timing,
        }
    }
}

#[test]
fn a_known_phone_gets_the_computer_name() {
    let phone_key = Identity::generate();
    let phone_public = phone_key.public;
    let world = World::new(vec![phone_key.public], None);
    let (mut mine, theirs) = pipes();
    let handle = phone(
        phone_key,
        vec![world.computer.identity.public],
        PhoneSays::Yes,
        theirs,
    );
    let ended = within(|stop| serve(&p(), &mut mine, &world.context(), false, stop));
    assert_eq!(ended, Ok(Ended::Closed));
    let events = handle.join().unwrap();
    assert!(
        matches!(events.last(), Some(Event::Message(Body::Hello(hello))) if hello.computer_name == "맥")
    );
    assert_eq!(
        world.notes.0.lock().unwrap()[0],
        ("p".to_owned(), Note::Connected { key: phone_public })
    );
}

#[test]
fn pairing_saves_the_key_and_model_when_both_agree() {
    let phone_key = Identity::generate();
    let phone_public = phone_key.public;
    let world = World::new(Vec::new(), Some(true));
    let (mut mine, theirs) = pipes();
    let handle = phone(phone_key, Vec::new(), PhoneSays::Yes, theirs);
    let ended = within(|stop| serve(&p(), &mut mine, &world.context(), true, stop));
    assert_eq!(ended, Ok(Ended::Paired));
    drop(mine);
    assert!(
        handle
            .join()
            .unwrap()
            .iter()
            .any(|e| matches!(e, Event::Paired { .. }))
    );
    assert_eq!(
        *world.computer.trusted.lock().unwrap(),
        [Paired {
            key: phone_public,
            name: "SM A716S".into(),
            serial: "p".into()
        }]
    );
}

fn iphone() -> Phone {
    Phone::usb("ios:1", Some("iPhone".into()), None)
}

#[test]
fn pairing_saves_the_model_the_phone_tells() {
    let phone_key = Identity::generate();
    let world = World::new(Vec::new(), Some(true));
    let (mut mine, theirs) = pipes();
    let handle = named_phone(phone_key, Vec::new(), PhoneSays::Yes, theirs, "iPad Air 4");
    let ended = within(|stop| serve(&iphone(), &mut mine, &world.context(), true, stop));
    assert_eq!(ended, Ok(Ended::Paired));
    drop(mine);
    handle.join().unwrap();
    assert_eq!(world.computer.trusted.lock().unwrap()[0].name, "iPad Air 4");
}

/// 저장된 이름(saved)으로 이미 아는 폰이 "iPad Air 4" 라고 알리며 붙은 뒤의 이름
fn renamed(saved: &str) -> String {
    let phone_key = Identity::generate();
    let world = World::new(Vec::new(), None);
    world.computer.trust(phone_key.public, saved, "ios:1").unwrap();
    let (mut mine, theirs) = pipes();
    let handle = named_phone(
        phone_key,
        vec![world.computer.identity.public],
        PhoneSays::Yes,
        theirs,
        "iPad Air 4",
    );
    let ended = within(|stop| serve(&iphone(), &mut mine, &world.context(), false, stop));
    assert_eq!(ended, Ok(Ended::Closed));
    handle.join().unwrap();
    let name = world.computer.trusted.lock().unwrap()[0].name.clone();
    name
}

#[test]
fn a_known_phone_replaces_only_an_automatic_name() {
    assert_eq!(renamed("iPhone"), "iPad Air 4", "USB 목록이 붙인 이름");
    assert_eq!(renamed(""), "iPad Air 4");
    assert_eq!(renamed("ios:1"), "iPad Air 4", "일련번호");
    assert_eq!(renamed("내 패드"), "내 패드", "사람이 붙인 이름은 그대로");
}

#[test]
fn pairing_saves_nothing_when_someone_says_no() {
    for (computer, phone_says) in [(Some(false), PhoneSays::Yes), (Some(true), PhoneSays::No)] {
        let phone_key = Identity::generate();
        let world = World::new(Vec::new(), computer);
        let (mut mine, theirs) = pipes();
        let handle = phone(phone_key, Vec::new(), phone_says, theirs);
        let ended = within(|stop| serve(&p(), &mut mine, &world.context(), true, stop));
        assert!(matches!(ended, Err(ServeError::Session(_))), "{ended:?}");
        assert!(ended.unwrap_err().is_final());
        drop(mine);
        handle.join().unwrap();
        assert!(world.computer.keys().is_empty());
    }
}

#[test]
fn gives_up_when_nobody_checks_the_code() {
    let world = World::new(Vec::new(), None);
    let (mut mine, theirs) = pipes();
    let handle = phone(Identity::generate(), Vec::new(), PhoneSays::Yes, theirs);
    let ended = within(|stop| serve(&p(), &mut mine, &world.context(), true, stop));
    assert_eq!(ended, Err(ServeError::NoAnswer));
    drop(mine);
    handle.join().unwrap();
}

#[test]
fn gives_up_when_only_the_computer_answers() {
    let world = World::new(Vec::new(), Some(true));
    let (mut mine, theirs) = pipes();
    let handle = phone(Identity::generate(), Vec::new(), PhoneSays::Ignore, theirs);
    let ended = within(|stop| serve(&p(), &mut mine, &world.context(), true, stop));
    assert_eq!(ended, Err(ServeError::NoAnswer));
    drop(mine);
    handle.join().unwrap();
    assert!(world.computer.keys().is_empty());
}

#[test]
fn a_silent_phone_is_dropped() {
    let phone_key = Identity::generate();
    let mut world = World::new(vec![phone_key.public], None);
    // 이 기기는 정말 아무것도 보내지 않으므로 짧은 한계여도 흔들리지 않는다.
    world.timing.silence = Duration::from_millis(150);
    let (mut mine, theirs) = pipes();
    let handle = phone(phone_key, Vec::new(), PhoneSays::Mute, theirs);
    let ended = within(|stop| serve(&p(), &mut mine, &world.context(), false, stop));
    assert_eq!(ended, Err(ServeError::Silent));
    drop(mine);
    handle.join().unwrap();
}

#[test]
fn forgetting_a_phone_drops_it_at_the_next_ping() {
    let phone_key = Identity::generate();
    let world = World::new(vec![phone_key.public], None);
    let (mut mine, theirs) = pipes();
    let handle = phone(
        phone_key.clone(),
        vec![world.computer.identity.public],
        PhoneSays::Stay,
        theirs,
    );
    let ended = thread::scope(|scope| {
        scope.spawn(|| {
            thread::sleep(Duration::from_millis(80));
            world.computer.forget(phone_key.public).unwrap();
        });
        within(|stop| serve(&p(), &mut mine, &world.context(), false, stop))
    });
    assert_eq!(ended, Err(ServeError::Forgotten));
    assert!(ended.unwrap_err().is_final());
    drop(mine);
    handle.join().unwrap();
}

/// 폰 앱 하나를 흉내 낸다. 접속할 때마다 새 폰 쪽 세션을 띄운다.
struct OnePhone {
    identity: Identity,
    trusted: Vec<[u8; KEY_LEN]>,
    connects: AtomicUsize,
    phones: Vec<Phone>,
    handles: Mutex<Vec<JoinHandle<Vec<Event>>>>,
}

impl OnePhone {
    fn new(identity: Identity, trusted: Vec<[u8; KEY_LEN]>, phone: Phone) -> Self {
        Self {
            identity,
            trusted,
            connects: AtomicUsize::new(0),
            phones: vec![phone],
            handles: Mutex::default(),
        }
    }
}

impl PhoneLinks for OnePhone {
    fn link(&self) -> Link {
        self.phones[0].link
    }

    fn attached(&self) -> Result<Vec<Phone>, PortError> {
        Ok(self.phones.clone())
    }

    fn watch(&self, changed: &mut dyn FnMut(Vec<Phone>)) -> Result<(), PortError> {
        changed(self.phones.clone());
        thread::sleep(Duration::from_millis(400));
        Err(PortError::new("adb 끊김"))
    }

    fn connect(&self, _id: &str) -> Result<Box<dyn Conn>, PortError> {
        self.connects.fetch_add(1, Ordering::Relaxed);
        let (mine, theirs) = pipes();
        let handle = phone(
            self.identity.clone(),
            self.trusted.clone(),
            PhoneSays::Yes,
            theirs,
        );
        self.handles.lock().unwrap().push(handle);
        Ok(Box::new(mine))
    }
}

/// 연결 하나를 돌리되 정해진 시간 안에 끝나지 않으면 멈춘다. 기대한 끝(등록 해제, 답 없음 등)을
/// 놓치는 버그가 끝없는 매달림이 아니라 Ok(Stopped) 같은 분명한 실패로 드러나게 한다.
fn within<T>(run: impl FnOnce(&AtomicBool) -> T) -> T {
    const LIMIT: Duration = Duration::from_secs(10);
    let stop = AtomicBool::new(false);
    let done = AtomicBool::new(false);
    thread::scope(|scope| {
        scope.spawn(|| {
            let deadline = std::time::Instant::now() + LIMIT;
            while !done.load(Ordering::Relaxed) && std::time::Instant::now() < deadline {
                thread::sleep(Duration::from_millis(20));
            }
            stop.store(true, Ordering::Relaxed);
        });
        let ended = run(&stop);
        done.store(true, Ordering::Relaxed);
        ended
    })
}

/// 느린 CI 서버를 생각해 고정 시간 대신 조건이 될 때까지 기다린다(최대 20초). 됐으면 true.
fn wait_for(done: impl Fn() -> bool) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while !done() {
        if std::time::Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(50));
    }
    true
}

fn noted(world: &World, matches: impl Fn(&Note) -> bool) -> bool {
    world
        .notes
        .0
        .lock()
        .unwrap()
        .iter()
        .any(|(_, note)| matches(note))
}

/// `keep_connected` 를 돌리면서 `during` 을 부르고, 끝나면 멈춘다.
fn run_while(links: &OnePhone, world: &World, during: impl FnOnce()) {
    let stop = AtomicBool::new(false);
    thread::scope(|scope| {
        scope.spawn(|| keep_connected(&[links], &world.context(), &stop));
        during();
        stop.store(true, Ordering::Relaxed);
    });
}

#[test]
fn a_stranger_is_refused_once_and_left_alone() {
    let links = OnePhone::new(Identity::generate(), Vec::new(), p());
    let world = World::new(Vec::new(), None);
    run_while(&links, &world, || {
        wait_for(|| noted(&world, |note| matches!(note, Note::Refused(_))));
        // 거절된 뒤 다시 붙지 않는지 잠깐 더 본다.
        thread::sleep(Duration::from_millis(1000));
    });
    assert_eq!(links.connects.load(Ordering::Relaxed), 1);
    let notes = world.notes.0.lock().unwrap();
    assert!(
        matches!(&notes[..], [(_, Note::Refused(_)), ..]),
        "{notes:?}"
    );
}

#[test]
fn reconnects_after_the_phone_closes() {
    let phone_key = Identity::generate();
    let world = World::new(vec![phone_key.public], None);
    let links = OnePhone::new(phone_key, vec![world.computer.identity.public], p());
    run_while(&links, &world, || {
        wait_for(|| {
            world
                .notes
                .0
                .lock()
                .unwrap()
                .iter()
                .filter(|(_, note)| matches!(note, Note::Connected { .. }))
                .count()
                >= 2
        });
    });
    let connects = links.connects.load(Ordering::Relaxed);
    assert!(connects >= 2);
    let notes = world.notes.0.lock().unwrap();
    assert_eq!(
        notes
            .iter()
            .filter(|(_, note)| matches!(note, Note::Connected { .. }))
            .count(),
        connects
    );
}

#[test]
fn a_blocked_phone_is_reported_and_not_connected() {
    let blocked = Phone {
        blocked: Some("허용 안 됨".into()),
        ..p()
    };
    let links = OnePhone::new(Identity::generate(), Vec::new(), blocked);
    let world = World::new(Vec::new(), None);
    run_while(&links, &world, || {
        wait_for(|| !world.notes.0.lock().unwrap().is_empty());
    });
    assert_eq!(links.connects.load(Ordering::Relaxed), 0);
    assert_eq!(
        world.notes.0.lock().unwrap()[0],
        ("p".to_owned(), Note::Lost("허용 안 됨".into()))
    );
}

#[test]
fn a_refused_phone_pairs_when_asked() {
    let links = OnePhone::new(Identity::generate(), Vec::new(), p());
    let world = World::new(Vec::new(), Some(true));
    run_while(&links, &world, || {
        thread::sleep(Duration::from_millis(300));
        assert!(world.computer.keys().is_empty());
        world.requests.ask("p");
        wait_for(|| {
            noted(&world, |note| matches!(note, Note::Paired { .. }))
                && world
                    .notes
                    .0
                    .lock()
                    .unwrap()
                    .iter()
                    .skip_while(|(_, note)| !matches!(note, Note::Paired { .. }))
                    .any(|(_, note)| matches!(note, Note::Connected { .. }))
        });
    });
    assert_eq!(world.computer.keys(), [links.identity.public]);
    assert!(!world.requests.is_asked("p"), "요청은 쓰고 나면 지운다");
    let notes = world.notes.0.lock().unwrap();
    let after: Vec<_> = notes
        .iter()
        .skip_while(|(_, note)| !matches!(note, Note::Paired { .. }))
        .take(2)
        .collect();
    assert!(
        matches!(
            after[..],
            [(_, Note::Paired { .. }), (_, Note::Connected { .. })]
        ),
        "짝 맺은 뒤 끊김 없이 연결: {after:?}"
    );
}

/// 앱이 꺼져 있다가 나중에 열리는 폰
struct LateApp {
    inner: OnePhone,
    opens_after: usize,
}

impl PhoneLinks for LateApp {
    fn link(&self) -> Link {
        Link::Usb
    }

    fn attached(&self) -> Result<Vec<Phone>, PortError> {
        self.inner.attached()
    }

    fn watch(&self, changed: &mut dyn FnMut(Vec<Phone>)) -> Result<(), PortError> {
        self.inner.watch(changed)
    }

    fn connect(&self, id: &str) -> Result<Box<dyn Conn>, PortError> {
        if self.inner.connects.fetch_add(1, Ordering::Relaxed) < self.opens_after {
            return Err(PortError::new("폰의 앱이 꺼져 있거나 뒤에 있음"));
        }
        self.inner.connects.fetch_sub(1, Ordering::Relaxed);
        self.inner.connect(id)
    }
}

#[test]
fn a_pair_request_waits_for_the_app_to_open() {
    let links = LateApp {
        inner: OnePhone::new(Identity::generate(), Vec::new(), p()),
        opens_after: 2,
    };
    let world = World::new(Vec::new(), Some(true));
    world.requests.ask("p");
    let stop = AtomicBool::new(false);
    thread::scope(|scope| {
        scope.spawn(|| keep_connected(&[&links], &world.context(), &stop));
        // 다시 시도 간격(0.5초, 1초)에 짝 맺기까지 더해지므로 느린 CI 서버를 생각해 넉넉히 기다린다.
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while world.computer.keys().is_empty() && std::time::Instant::now() < deadline {
            thread::sleep(Duration::from_millis(100));
        }
        stop.store(true, Ordering::Relaxed);
    });
    assert_eq!(world.computer.keys(), [links.inner.identity.public]);
}

/// 목록을 시험 중에 바꿀 수 있는 링크. 접속할 때마다 폰 쪽 세션을 새로 띄운다.
struct Switch {
    link: Link,
    /// 출처 이름. None 이면 길의 기본 이름
    name: Option<&'static str>,
    phones: Mutex<Vec<Phone>>,
    identity: Identity,
    trusted: Vec<[u8; KEY_LEN]>,
    says: PhoneSays,
    connects: AtomicUsize,
    strict: bool,
}

impl Switch {
    fn new(link: Link, phone: Phone, identity: Identity, trusted: Vec<[u8; KEY_LEN]>) -> Self {
        Self {
            link,
            name: None,
            phones: Mutex::new(vec![phone]),
            identity,
            trusted,
            says: PhoneSays::Stay,
            connects: AtomicUsize::new(0),
            strict: false,
        }
    }

    fn unplug(&self) {
        self.phones.lock().unwrap().clear();
    }

    fn plug(&self, phone: Phone) {
        *self.phones.lock().unwrap() = vec![phone];
    }
}

impl PhoneLinks for Switch {
    fn link(&self) -> Link {
        self.link
    }

    fn source(&self) -> Source {
        self.name
            .map_or(Source::of(self.link), |name| Source::new(self.link, name))
    }

    fn attached(&self) -> Result<Vec<Phone>, PortError> {
        Ok(self.phones.lock().unwrap().clone())
    }

    fn watch(&self, changed: &mut dyn FnMut(Vec<Phone>)) -> Result<(), PortError> {
        let mut last = None;
        for _ in 0..250 {
            let now = self.phones.lock().unwrap().clone();
            if last.as_ref() != Some(&now) {
                changed(now.clone());
                last = Some(now);
            }
            thread::sleep(Duration::from_millis(20));
        }
        Ok(())
    }

    fn connect(&self, _id: &str) -> Result<Box<dyn Conn>, PortError> {
        self.connects.fetch_add(1, Ordering::Relaxed);
        let (mut mine, theirs) = pipes();
        mine.strict = self.strict;
        phone(
            self.identity.clone(),
            self.trusted.clone(),
            self.says,
            theirs,
        );
        Ok(Box::new(mine))
    }
}

/// 시험 중 단언이 실패해도 멈추게 한다.
struct StopOnDrop<'a>(&'a AtomicBool);

impl Drop for StopOnDrop<'_> {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

fn run_links(links: &[&dyn PhoneLinks], world: &World, during: impl FnOnce()) {
    let stop = AtomicBool::new(false);
    thread::scope(|scope| {
        scope.spawn(|| keep_connected(links, &world.context(), &stop));
        let _stop = StopOnDrop(&stop);
        during();
    });
}

fn connected_on(world: &World, id: &str) -> usize {
    world
        .notes
        .0
        .lock()
        .unwrap()
        .iter()
        .filter(|(phone, note)| phone == id && matches!(note, Note::Connected { .. }))
        .count()
}

#[test]
fn bluetooth_waits_for_usb_and_takes_over_after_unplug() {
    let phone_key = Identity::generate();
    let public = phone_key.public;
    let world = World::new(vec![public], None);
    let computer = vec![world.computer.identity.public];
    let usb = Switch::new(Link::Usb, p(), phone_key.clone(), computer.clone());
    let air = Switch::new(Link::Ble, ble(&public), phone_key, computer);
    // 블루투스 알림 신호는 USB 가 붙은 뒤에 보이게 한다.
    air.unplug();
    run_links(&[&usb, &air], &world, || {
        wait_for(|| connected_on(&world, "p") == 1);
        air.plug(ble(&public));
        thread::sleep(Duration::from_millis(1500));
        assert_eq!(connected_on(&world, "p"), 1);
        assert_eq!(
            air.connects.load(Ordering::Relaxed),
            0,
            "USB 가 있으면 블루투스로 붙지 않음"
        );
        usb.unplug();
        wait_for(|| connected_on(&world, "ble:1") == 1);
    });
    assert_eq!(connected_on(&world, "ble:1"), 1, "뺀 뒤 블루투스로 붙음");
}

#[test]
fn bluetooth_ignores_an_unregistered_beacon() {
    let stranger = Identity::generate();
    let world = World::new(Vec::new(), None);
    let air = Switch::new(Link::Ble, ble(&stranger.public), stranger, Vec::new());
    run_links(&[&air], &world, || {
        thread::sleep(Duration::from_millis(1200))
    });
    assert_eq!(air.connects.load(Ordering::Relaxed), 0);
    assert!(world.notes.0.lock().unwrap().is_empty());
}

#[test]
fn bluetooth_reports_busy_when_the_phone_drops_it_at_ready() {
    let phone_key = Identity::generate();
    let public = phone_key.public;
    let world = World::new(vec![public], None);
    let mut air = Switch::new(
        Link::Ble,
        ble(&public),
        phone_key,
        vec![world.computer.identity.public],
    );
    air.says = PhoneSays::Busy;
    air.strict = true;
    // 멈추는 동안(부하가 크면 오래 걸림)의 재시도는 빼고, 지켜본 시간 안의 접속만 센다.
    let (mut before, mut seen) = (0, 0);
    run_links(&[&air], &world, || {
        let busy = ServeError::Busy.to_string();
        wait_for(|| {
            noted(
                &world,
                |note| matches!(note, Note::Lost(reason) if reason == &busy),
            )
        });
        // 거절을 본 순간부터 2.5초 더 본다. 보통 재시도(1초)였다면 이 사이 두 번 붙는다.
        before = air.connects.load(Ordering::Relaxed);
        thread::sleep(Duration::from_millis(2500));
        seen = air.connects.load(Ordering::Relaxed);
    });
    // 5초 쉬므로 0 이어야 한다. 굶어서 알림이 늦게 보인 경우만 생각해 1 까지 둔다.
    assert!(
        seen - before <= 1,
        "거절된 뒤에는 5초 쉬고 다시 시도 {before} → {seen} {:?}",
        world.notes.0.lock().unwrap()
    );
    assert_eq!(
        connected_on(&world, "ble:1"),
        0,
        "받아들이기 전에는 연결됨을 알리지 않음"
    );
    assert!(world.notes.0.lock().unwrap().iter().any(
        |(_, note)| matches!(note, Note::Lost(reason) if reason == &ServeError::Busy.to_string())
    ));
}

#[test]
fn bluetooth_waits_for_the_list_to_change_after_a_refusal() {
    let phone_key = Identity::generate();
    let public = phone_key.public;
    let world = World::new(vec![public], None);
    // 기기는 이 컴퓨터를 모른다.
    let air = Switch::new(Link::Ble, ble(&public), phone_key, Vec::new());
    run_links(&[&air], &world, || {
        wait_for(|| air.connects.load(Ordering::Relaxed) >= 1);
        // 목록이 그대로면 다시 붙지 않는지 잠깐 더 본다.
        thread::sleep(Duration::from_millis(1000));
        assert_eq!(air.connects.load(Ordering::Relaxed), 1);
        world.computer.trust([7; KEY_LEN], "다른 기기", "").unwrap();
        wait_for(|| air.connects.load(Ordering::Relaxed) >= 2);
        thread::sleep(Duration::from_millis(300));
    });
    assert_eq!(
        air.connects.load(Ordering::Relaxed),
        2,
        "등록 목록이 바뀌면 다시 시도"
    );
}

#[test]
fn bluetooth_fills_a_missing_name_but_keeps_the_serial() {
    let phone_key = Identity::generate();
    let public = phone_key.public;
    let world = World::new(Vec::new(), None);
    world.computer.trust(public, "", "R3C").unwrap();
    let (mine, theirs) = pipes();
    let handle = phone(
        phone_key,
        vec![world.computer.identity.public],
        PhoneSays::Stay,
        theirs,
    );
    let mut mine = mine;
    let stop = AtomicBool::new(false);
    let result = thread::scope(|scope| {
        scope.spawn(|| {
            thread::sleep(Duration::from_millis(300));
            stop.store(true, Ordering::Relaxed);
        });
        serve(&ble(&public), &mut mine, &world.context(), false, &stop)
    });
    drop(mine);
    handle.join().unwrap();
    assert!(result.is_ok(), "{result:?}");
    let saved = world.computer.trusted().unwrap();
    assert_eq!(saved[0].name, "Galaxy A71");
    assert_eq!(saved[0].serial, "R3C");
}

#[test]
fn usb_wins_the_slot_over_bluetooth() {
    let active = Active::default();
    let key = [1; KEY_LEN];
    assert!(active.claim(key, Link::Usb, "p"));
    assert!(!active.claim(key, Link::Ble, "ble:1"));
    active.release(key, "ble:1");
    assert!(
        active.holds(key, Link::Usb),
        "블루투스가 USB 자리를 놓지 않음"
    );
    active.release(key, "p");
    assert!(active.claim(key, Link::Ble, "ble:1"));
    assert!(
        !active.claim(key, Link::Ble, "ble:2"),
        "주소가 바뀐 같은 기기는 블루투스 연결을 하나만"
    );
    assert!(
        active.claim(key, Link::Usb, "p"),
        "USB 는 블루투스 자리를 가져감"
    );
    active.release(key, "ble:1");
    assert!(
        active.holds(key, Link::Usb),
        "끝난 블루투스가 USB 자리를 지우지 않음"
    );
}

#[test]
fn bluetooth_yields_when_usb_already_holds_the_phone() {
    let phone_key = Identity::generate();
    let public = phone_key.public;
    let world = World::new(vec![public], None);
    assert!(world.active.claim(public, Link::Usb, "p"));
    let (mut mine, theirs) = pipes();
    let handle = phone(
        phone_key,
        vec![world.computer.identity.public],
        PhoneSays::Stay,
        theirs,
    );
    let result = within(|stop| serve(&ble(&public), &mut mine, &world.context(), false, stop));
    drop(mine);
    handle.join().unwrap();
    assert_eq!(result, Ok(Ended::Yielded));
    assert!(
        world.notes.0.lock().unwrap().is_empty(),
        "물러날 때는 알리지 않음"
    );
}

/// 주소가 바뀐 같은 기기: 옛 연결이 살아 있는 동안 새 주소로 붙지 않고, 옛 연결이 끝나야 넘겨받는다.
/// 침묵 한계가 빡빡하면 느린 서버에서 옛 연결이 굶어 먼저 끊겨(기기가 응답하지 않음) 새 주소가 정상으로
/// 붙으므로, World 의 넉넉한 침묵 한계에 기댄다.
#[test]
fn a_rotated_bluetooth_address_does_not_add_a_second_connection() {
    let phone_key = Identity::generate();
    let public = phone_key.public;
    let world = World::new(vec![public], None);
    let computer = vec![world.computer.identity.public];
    let second = || Phone {
        id: "ble:2".into(),
        ..ble(&public)
    };
    let old = Switch::new(Link::Ble, ble(&public), phone_key.clone(), computer.clone());
    let new = Switch::new(Link::Ble, second(), phone_key, computer);
    new.unplug();
    let notes = || format!("{:?}", world.notes.0.lock().unwrap());
    // 멈추는 동안(부하가 크면 오래 걸림)의 재접속은 빼고, 지켜본 시간 안의 값으로 단정한다.
    let mut taken = (0, 0);
    run_links(&[&old, &new], &world, || {
        wait_for(|| connected_on(&world, "ble:1") > 0);
        new.plug(second());
        // 재시도 간격(RETRY)이 몇 번 지나도록 둔다.
        thread::sleep(sidedesk_domain::RETRY * 2 + Duration::from_millis(500));
        assert_eq!(connected_on(&world, "ble:1"), 1, "{}", notes());
        assert_eq!(
            new.connects.load(Ordering::Relaxed),
            0,
            "옛 연결이 살아 있는 동안 새 주소로 붙음: {}",
            notes()
        );
        // 옛 주소가 사라지면 옛 연결이 끝나 자리를 놓고, 새 주소가 넘겨받는다.
        old.unplug();
        wait_for(|| connected_on(&world, "ble:2") > 0);
        taken = (
            connected_on(&world, "ble:2"),
            new.connects.load(Ordering::Relaxed),
        );
    });
    assert_eq!(taken, (1, 1), "넘겨받기(연결됨, 접속 수): {}", notes());
}

#[test]
fn each_usb_source_reports_only_its_own_list() {
    let phone_key = Identity::generate();
    let world = World::new(vec![phone_key.public], None);
    let computer = vec![world.computer.identity.public];
    let mut adb = Switch::new(Link::Usb, p(), phone_key.clone(), computer.clone());
    adb.name = Some("adb");
    adb.unplug();
    let mut mux = Switch::new(
        Link::Usb,
        Phone::usb("ios:1", Some("iPhone".into()), None),
        phone_key,
        computer,
    );
    mux.name = Some("usbmux");
    run_links(&[&adb, &mux], &world, || {
        wait_for(|| {
            let lists = world.notes.1.lock().unwrap();
            lists.iter().any(|(source, _)| source.name == "adb")
                && lists.iter().any(|(source, _)| source.name == "usbmux")
        });
    });
    let lists = world.notes.1.lock().unwrap().clone();
    assert!(lists.contains(&(Source::new(Link::Usb, "adb"), Vec::new())));
    assert!(lists.contains(&(Source::new(Link::Usb, "usbmux"), vec!["ios:1".to_owned()])));
    assert!(
        lists
            .iter()
            .all(|(source, ids)| source.name != "adb" || ids.is_empty()),
        "adb 목록에 아이폰이 섞이지 않음"
    );
}
