//! 실제 폰 대신 공용 코어의 폰 쪽 세션을 스레드에서 돌려 컴퓨터 쪽을 시험한다.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use sidedesk_core::{Body, Event, Identity, KEY_LEN, Role, Session};
use sidedesk_domain::{
    ComputerName, Conn, Context, Ended, Keys, Note, PairRequests, Paired, Phone, PhoneLinks,
    PortError, Prompt, Received, Report, ServeError, Timing, keep_connected, serve,
};

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
}

/// 폰 쪽 세션. 컴퓨터 이름을 받으면 연결을 닫는다(Stay 는 빼고).
fn phone(
    identity: Identity,
    trusted: Vec<[u8; KEY_LEN]>,
    says: PhoneSays,
    mut pipe: Pipe,
) -> JoinHandle<Vec<Event>> {
    thread::spawn(move || {
        let mut session = Session::new(Role::Phone, &identity, trusted, false);
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
    Phone {
        id: "p".into(),
        model: Some("SM A716S".into()),
        blocked: None,
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
struct Notes(Mutex<Vec<(String, Note)>>);

impl Report for Notes {
    fn note(&self, phone: &str, note: Note) {
        self.0.lock().unwrap().push((phone.to_owned(), note));
    }
}

struct World {
    computer: MemoryKeys,
    requests: PairRequests,
    person: Person,
    notes: Notes,
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
            computer: MemoryKeys {
                identity: Identity::generate(),
                trusted: Mutex::new(trusted),
            },
            requests: PairRequests::default(),
            person: Person(answer, Mutex::default()),
            notes: Notes::default(),
            timing: Timing {
                ping: Duration::from_millis(20),
                silence: Duration::from_millis(150),
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
    let ended = serve(
        &p(),
        &mut mine,
        &world.context(),
        false,
        &AtomicBool::new(false),
    );
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
    let ended = serve(
        &p(),
        &mut mine,
        &world.context(),
        true,
        &AtomicBool::new(false),
    );
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

#[test]
fn pairing_saves_nothing_when_someone_says_no() {
    for (computer, phone_says) in [(Some(false), PhoneSays::Yes), (Some(true), PhoneSays::No)] {
        let phone_key = Identity::generate();
        let world = World::new(Vec::new(), computer);
        let (mut mine, theirs) = pipes();
        let handle = phone(phone_key, Vec::new(), phone_says, theirs);
        let ended = serve(
            &p(),
            &mut mine,
            &world.context(),
            true,
            &AtomicBool::new(false),
        );
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
    let ended = serve(
        &p(),
        &mut mine,
        &world.context(),
        true,
        &AtomicBool::new(false),
    );
    assert_eq!(ended, Err(ServeError::NoAnswer));
    drop(mine);
    handle.join().unwrap();
}

#[test]
fn gives_up_when_only_the_computer_answers() {
    let world = World::new(Vec::new(), Some(true));
    let (mut mine, theirs) = pipes();
    let handle = phone(Identity::generate(), Vec::new(), PhoneSays::Ignore, theirs);
    let ended = serve(
        &p(),
        &mut mine,
        &world.context(),
        true,
        &AtomicBool::new(false),
    );
    assert_eq!(ended, Err(ServeError::NoAnswer));
    drop(mine);
    handle.join().unwrap();
    assert!(world.computer.keys().is_empty());
}

#[test]
fn a_silent_phone_is_dropped() {
    let phone_key = Identity::generate();
    let world = World::new(vec![phone_key.public], None);
    let (mut mine, theirs) = pipes();
    let handle = phone(phone_key, Vec::new(), PhoneSays::Mute, theirs);
    let ended = serve(
        &p(),
        &mut mine,
        &world.context(),
        false,
        &AtomicBool::new(false),
    );
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
        serve(
            &p(),
            &mut mine,
            &world.context(),
            false,
            &AtomicBool::new(false),
        )
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

/// `keep_connected` 를 돌리면서 `during` 을 부르고, 끝나면 멈춘다.
fn run_while(links: &OnePhone, world: &World, during: impl FnOnce()) {
    let stop = AtomicBool::new(false);
    thread::scope(|scope| {
        scope.spawn(|| keep_connected(links, &world.context(), &stop));
        during();
        stop.store(true, Ordering::Relaxed);
    });
}

#[test]
fn a_stranger_is_refused_once_and_left_alone() {
    let links = OnePhone::new(Identity::generate(), Vec::new(), p());
    let world = World::new(Vec::new(), None);
    run_while(&links, &world, || {
        thread::sleep(Duration::from_millis(1500))
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
        thread::sleep(Duration::from_millis(2300))
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
    run_while(&links, &world, || thread::sleep(Duration::from_millis(300)));
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
        thread::sleep(Duration::from_millis(1500));
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
        scope.spawn(|| keep_connected(&links, &world.context(), &stop));
        thread::sleep(Duration::from_millis(3000));
        stop.store(true, Ordering::Relaxed);
    });
    assert_eq!(world.computer.keys(), [links.inner.identity.public]);
}
