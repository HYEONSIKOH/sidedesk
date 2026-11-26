//! 소켓을 모르는 연결 상태 기계. 컴퓨터와 폰이 같이 쓴다.
//!
//! 순서: 머리말(버전) → Noise XX 세 번 → 서로 아는 사이인지 알림 → (짝 맺기) → 준비됨.

use std::collections::VecDeque;

use prost::Message;
use sha2::{Digest, Sha256};
use snow::{Builder, HandshakeState, TransportState};

use crate::proto::envelope::Body;
use crate::proto::{Answer, Envelope, Intro, Pairing};
use crate::wire::{FrameReader, MAX_BODY, frame};

/// 큰 버전이 다르면 붙지 않는다.
pub const VERSION: [u8; 2] = [1, 0];
pub const KEY_LEN: usize = 32;
const MAGIC: &[u8; 2] = b"SD";
const PATTERN: &str = "Noise_XX_25519_ChaChaPoly_BLAKE2s";
const PROLOGUE: &[u8] = b"sidedesk";
const NONCE_LEN: usize = 32;
const TAG_LEN: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// 접속하는 쪽. Noise 를 시작하고 짝 맺기를 연다.
    Computer,
    Phone,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SessionError {
    #[error(
        "버전이 달라 연결할 수 없습니다. 두 앱을 최신 버전으로 업데이트하세요 (내 버전 {mine}, 상대 {theirs})"
    )]
    Version { mine: u8, theirs: u8 },
    #[error("등록되지 않은 기기입니다. 등록을 눌러 다시 등록하세요")]
    NotPaired,
    #[error("등록을 취소함")]
    Rejected,
    #[error("읽을 수 없는 연결: {0}")]
    Broken(&'static str),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// 양쪽에 같은 숫자가 떴는지 사람이 확인해야 한다. 확인은 `confirm`.
    PairingCode(String),
    /// 둘 다 맞다고 했다. 이 키를 저장한다.
    Paired {
        peer: [u8; KEY_LEN],
    },
    Ready {
        peer: [u8; KEY_LEN],
    },
    Message(Body),
}

#[derive(Clone)]
pub struct Identity {
    pub private: [u8; KEY_LEN],
    pub public: [u8; KEY_LEN],
}

impl Identity {
    pub fn generate() -> Self {
        let pair = Builder::new(params())
            .generate_keypair()
            .expect("난수를 얻지 못함");
        Self {
            private: pair.private.try_into().expect("키 길이"),
            public: pair.public.try_into().expect("키 길이"),
        }
    }
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Identity")
            .field("public", &self.public)
            .finish_non_exhaustive()
    }
}

fn params() -> snow::params::NoiseParams {
    PATTERN.parse().expect("Noise 이름")
}

enum Stage {
    Handshake,
    Intro,
    /// 컴퓨터: 약속을 보내고 폰의 난수를 기다림
    AwaitNonce,
    /// 폰: 컴퓨터의 약속을 기다림
    AwaitCommit,
    /// 폰: 난수를 보내고 공개를 기다림
    AwaitReveal {
        commit: Vec<u8>,
    },
    Answer {
        mine: Option<bool>,
        theirs: Option<bool>,
    },
    Ready,
}

pub struct Session {
    role: Role,
    private: [u8; KEY_LEN],
    trusted: Vec<[u8; KEY_LEN]>,
    wants_pairing: bool,
    allow_pairing: bool,
    stage: Stage,
    preface: Vec<u8>,
    reader: FrameReader,
    handshake: Option<Box<HandshakeState>>,
    transport: Option<TransportState>,
    hash: Vec<u8>,
    peer: [u8; KEY_LEN],
    mine: [u8; NONCE_LEN],
    theirs: [u8; NONCE_LEN],
    out: Vec<u8>,
    events: VecDeque<Event>,
    failed: Option<SessionError>,
}

impl Session {
    /// 폰 쪽에서 상대가 등록을 원해도 받지 않는다(블루투스 연결). 등록 요청은 `NotPaired` 로 끊는다.
    pub fn refusing_pairing(mut self) -> Self {
        self.allow_pairing = false;
        self
    }

    /// `wants_pairing` 은 컴퓨터만 쓴다. 폰은 컴퓨터가 열 때만 짝을 맺는다.
    pub fn new(
        role: Role,
        identity: &Identity,
        trusted: Vec<[u8; KEY_LEN]>,
        wants_pairing: bool,
    ) -> Self {
        Self {
            role,
            private: identity.private,
            trusted,
            wants_pairing: wants_pairing && role == Role::Computer,
            allow_pairing: true,
            stage: Stage::Handshake,
            preface: Vec::new(),
            reader: FrameReader::default(),
            handshake: None,
            transport: None,
            hash: Vec::new(),
            peer: [0; KEY_LEN],
            mine: [0; NONCE_LEN],
            theirs: [0; NONCE_LEN],
            out: preface().to_vec(),
            events: VecDeque::new(),
            failed: None,
        }
    }

    /// 받은 바이트를 넣는다. 보낼 것은 `take_output`, 일어난 일은 `next_event` 로 꺼낸다.
    pub fn feed(&mut self, bytes: &[u8]) -> Result<(), SessionError> {
        self.guard(|session| session.read(bytes))
    }

    pub fn take_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.out)
    }

    pub fn next_event(&mut self) -> Option<Event> {
        self.events.pop_front()
    }

    pub fn is_ready(&self) -> bool {
        self.failed.is_none() && matches!(self.stage, Stage::Ready)
    }

    /// 짝 맺기 숫자를 사람이 확인한 결과. 숫자가 뜬 때가 아니면 아무것도 안 한다.
    pub fn confirm(&mut self, same: bool) -> Result<(), SessionError> {
        self.guard(|session| {
            let Stage::Answer { mine: None, theirs } = session.stage else {
                return Ok(());
            };
            session.write(Body::PairAnswer(Answer { same }))?;
            if !same {
                return Err(SessionError::Rejected);
            }
            session.stage = Stage::Answer {
                mine: Some(true),
                theirs,
            };
            session.finish_pairing();
            Ok(())
        })
    }

    pub fn send(&mut self, body: Body) -> Result<(), SessionError> {
        self.guard(|session| {
            if !matches!(session.stage, Stage::Ready) {
                return Err(SessionError::Broken("아직 준비되지 않음"));
            }
            session.write(body)
        })
    }

    /// 살아 있다고 알린다. 암호 연결 전이면 아무것도 안 한다.
    pub fn ping(&mut self) -> Result<(), SessionError> {
        self.guard(|session| match session.transport {
            Some(_) => session.write(Body::Ping(Default::default())),
            None => Ok(()),
        })
    }

    fn guard(
        &mut self,
        work: impl FnOnce(&mut Self) -> Result<(), SessionError>,
    ) -> Result<(), SessionError> {
        if let Some(error) = &self.failed {
            return Err(error.clone());
        }
        work(self).inspect_err(|error| self.failed = Some(error.clone()))
    }

    fn read(&mut self, mut bytes: &[u8]) -> Result<(), SessionError> {
        if self.preface.len() < 4 {
            let take = bytes.len().min(4 - self.preface.len());
            self.preface.extend_from_slice(&bytes[..take]);
            bytes = &bytes[take..];
            if self.preface.len() < 4 {
                return Ok(());
            }
            if &self.preface[..2] != MAGIC {
                return Err(SessionError::Broken("SideDesk 연결이 아님"));
            }
            if self.preface[2] != VERSION[0] {
                return Err(SessionError::Version {
                    mine: VERSION[0],
                    theirs: self.preface[2],
                });
            }
            self.start_handshake()?;
        }
        self.reader.push(bytes);
        while let Some(body) = self
            .reader
            .pop()
            .map_err(|_| SessionError::Broken("메시지 길이가 틀림"))?
        {
            if self.handshake.is_some() {
                self.read_handshake(&body)?;
            } else {
                self.read_message(&body)?;
            }
        }
        Ok(())
    }

    /// 양쪽 머리말을 프롤로그에 넣어, 중간에서 버전을 바꾸면 핸드셰이크가 깨지게 한다.
    fn start_handshake(&mut self) -> Result<(), SessionError> {
        let (computer, phone) = match self.role {
            Role::Computer => (preface().to_vec(), self.preface.clone()),
            Role::Phone => (self.preface.clone(), preface().to_vec()),
        };
        let prologue = [PROLOGUE, &computer, &phone].concat();
        let builder = Builder::new(params())
            .local_private_key(&self.private)
            .and_then(|builder| builder.prologue(&prologue))
            .map_err(|_| SessionError::Broken("핸드셰이크 설정 실패"))?;
        let handshake = match self.role {
            Role::Computer => builder.build_initiator(),
            Role::Phone => builder.build_responder(),
        }
        .map_err(|_| SessionError::Broken("핸드셰이크 설정 실패"))?;
        self.handshake = Some(Box::new(handshake));
        if self.role == Role::Computer {
            self.write_handshake()?;
        }
        Ok(())
    }

    fn read_handshake(&mut self, body: &[u8]) -> Result<(), SessionError> {
        let handshake = self.handshake.as_mut().expect("핸드셰이크 중");
        let mut payload = vec![0; MAX_BODY];
        handshake
            .read_message(body, &mut payload)
            .map_err(|_| SessionError::Broken("핸드셰이크 실패"))?;
        if !handshake.is_handshake_finished() {
            self.write_handshake()?;
        }
        if self
            .handshake
            .as_ref()
            .is_some_and(|handshake| handshake.is_handshake_finished())
        {
            self.start_transport()?;
        }
        Ok(())
    }

    fn write_handshake(&mut self) -> Result<(), SessionError> {
        let handshake = self.handshake.as_mut().expect("핸드셰이크 중");
        let mut message = vec![0; MAX_BODY];
        let length = handshake
            .write_message(&[], &mut message)
            .map_err(|_| SessionError::Broken("핸드셰이크 실패"))?;
        self.out.extend_from_slice(&frame(&message[..length]));
        Ok(())
    }

    fn start_transport(&mut self) -> Result<(), SessionError> {
        let handshake = self.handshake.take().expect("핸드셰이크 중");
        self.hash = handshake.get_handshake_hash().to_vec();
        self.peer = handshake
            .get_remote_static()
            .and_then(|key| key.try_into().ok())
            .ok_or(SessionError::Broken("상대 키가 없음"))?;
        self.transport = Some(
            handshake
                .into_transport_mode()
                .map_err(|_| SessionError::Broken("핸드셰이크 실패"))?,
        );
        self.stage = Stage::Intro;
        let knows_you = self.trusted.contains(&self.peer);
        self.write(Body::Intro(Intro {
            knows_you,
            wants_pairing: self.wants_pairing,
        }))
    }

    fn read_message(&mut self, body: &[u8]) -> Result<(), SessionError> {
        let transport = self.transport.as_mut().expect("핸드셰이크 끝");
        let mut plain = vec![0; body.len()];
        let length = transport
            .read_message(body, &mut plain)
            .map_err(|_| SessionError::Broken("복호화 실패"))?;
        let body = Envelope::decode(&plain[..length])
            .ok()
            .and_then(|envelope| envelope.body)
            .ok_or(SessionError::Broken("읽을 수 없는 메시지"))?;
        self.handle(body)
    }

    fn handle(&mut self, body: Body) -> Result<(), SessionError> {
        match body {
            Body::Ping(_) => return self.write(Body::Pong(Default::default())),
            Body::Pong(_) => return Ok(()),
            _ => {}
        }
        let stage = std::mem::replace(&mut self.stage, Stage::Ready);
        match (stage, body) {
            (Stage::Intro, Body::Intro(intro)) => {
                let pairing = match self.role {
                    Role::Computer => self.wants_pairing,
                    Role::Phone => intro.wants_pairing,
                };
                if pairing && self.role == Role::Computer {
                    self.mine = nonce();
                    self.stage = Stage::AwaitNonce;
                    self.write(Body::PairCommit(Pairing {
                        value: commit(&self.mine),
                    }))
                } else if pairing && !self.allow_pairing {
                    Err(SessionError::NotPaired)
                } else if pairing {
                    self.stage = Stage::AwaitCommit;
                    Ok(())
                } else if intro.knows_you && self.trusted.contains(&self.peer) {
                    self.events.push_back(Event::Ready { peer: self.peer });
                    Ok(())
                } else {
                    Err(SessionError::NotPaired)
                }
            }
            (Stage::AwaitNonce, Body::PairNonce(nonce)) => {
                self.theirs = exact(nonce.value)?;
                self.write(Body::PairReveal(Pairing {
                    value: self.mine.to_vec(),
                }))?;
                self.show_code();
                Ok(())
            }
            (Stage::AwaitCommit, Body::PairCommit(promise)) => {
                self.mine = nonce();
                self.stage = Stage::AwaitReveal {
                    commit: promise.value,
                };
                self.write(Body::PairNonce(Pairing {
                    value: self.mine.to_vec(),
                }))
            }
            (Stage::AwaitReveal { commit: promise }, Body::PairReveal(reveal)) => {
                self.theirs = exact(reveal.value)?;
                if commit(&self.theirs) != promise {
                    return Err(SessionError::Broken("등록 확인값이 맞지 않음"));
                }
                self.show_code();
                Ok(())
            }
            (Stage::Answer { mine, theirs: None }, Body::PairAnswer(answer)) => {
                if !answer.same {
                    return Err(SessionError::Rejected);
                }
                self.stage = Stage::Answer {
                    mine,
                    theirs: Some(true),
                };
                self.finish_pairing();
                Ok(())
            }
            (Stage::Ready, body @ Body::Hello(_)) => {
                self.events.push_back(Event::Message(body));
                Ok(())
            }
            _ => Err(SessionError::Broken("순서가 틀린 메시지")),
        }
    }

    fn show_code(&mut self) {
        let (computer, phone) = match self.role {
            Role::Computer => (&self.mine, &self.theirs),
            Role::Phone => (&self.theirs, &self.mine),
        };
        let digest = Sha256::new()
            .chain_update(b"sidedesk code")
            .chain_update(&self.hash)
            .chain_update(computer)
            .chain_update(phone)
            .finalize();
        let number = u64::from_be_bytes(digest[..8].try_into().expect("8바이트")) % 1_000_000;
        self.events
            .push_back(Event::PairingCode(format!("{number:06}")));
        self.stage = Stage::Answer {
            mine: None,
            theirs: None,
        };
    }

    fn finish_pairing(&mut self) {
        if let Stage::Answer {
            mine: Some(true),
            theirs: Some(true),
        } = self.stage
        {
            self.stage = Stage::Ready;
            self.trusted.push(self.peer);
            self.events.push_back(Event::Paired { peer: self.peer });
            self.events.push_back(Event::Ready { peer: self.peer });
        }
    }

    fn write(&mut self, body: Body) -> Result<(), SessionError> {
        let plain = Envelope { body: Some(body) }.encode_to_vec();
        if plain.len() + TAG_LEN > MAX_BODY {
            return Err(SessionError::Broken("보낼 메시지가 너무 큼"));
        }
        let transport = self.transport.as_mut().expect("핸드셰이크 끝");
        let mut message = vec![0; plain.len() + TAG_LEN];
        let length = transport
            .write_message(&plain, &mut message)
            .map_err(|_| SessionError::Broken("암호화 실패"))?;
        self.out.extend_from_slice(&frame(&message[..length]));
        Ok(())
    }
}

fn preface() -> [u8; 4] {
    [MAGIC[0], MAGIC[1], VERSION[0], VERSION[1]]
}

fn nonce() -> [u8; NONCE_LEN] {
    let mut nonce = [0; NONCE_LEN];
    getrandom::fill(&mut nonce).expect("난수를 얻지 못함");
    nonce
}

fn commit(nonce: &[u8]) -> Vec<u8> {
    Sha256::new()
        .chain_update(b"sidedesk commit")
        .chain_update(nonce)
        .finalize()
        .to_vec()
}

fn exact(value: Vec<u8>) -> Result<[u8; NONCE_LEN], SessionError> {
    value
        .try_into()
        .map_err(|_| SessionError::Broken("등록 값의 길이가 틀림"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::Hello;

    struct Pair {
        computer: Session,
        phone: Session,
        computer_events: Vec<Event>,
        phone_events: Vec<Event>,
    }

    impl Pair {
        fn new(computer: Session, phone: Session) -> Self {
            let mut pair = Self {
                computer,
                phone,
                computer_events: Vec::new(),
                phone_events: Vec::new(),
            };
            let _ = pair.pump();
            pair
        }

        /// 양쪽이 더 보낼 것이 없을 때까지 주고받는다. 처음 난 오류를 돌려준다.
        fn pump(&mut self) -> Result<(), (Role, SessionError)> {
            loop {
                let to_phone = self.computer.take_output();
                let to_computer = self.phone.take_output();
                if to_phone.is_empty() && to_computer.is_empty() {
                    return Ok(());
                }
                let a = self.phone.feed(&to_phone).map_err(|e| (Role::Phone, e));
                let b = self
                    .computer
                    .feed(&to_computer)
                    .map_err(|e| (Role::Computer, e));
                self.computer_events
                    .extend(std::iter::from_fn(|| self.computer.next_event()));
                self.phone_events
                    .extend(std::iter::from_fn(|| self.phone.next_event()));
                a?;
                b?;
            }
        }

        fn code(events: &[Event]) -> Option<String> {
            events.iter().find_map(|event| match event {
                Event::PairingCode(code) => Some(code.clone()),
                _ => None,
            })
        }
    }

    fn keys() -> (Identity, Identity) {
        (Identity::generate(), Identity::generate())
    }

    fn known(computer: &Identity, phone: &Identity) -> Pair {
        Pair::new(
            Session::new(Role::Computer, computer, vec![phone.public], false),
            Session::new(Role::Phone, phone, vec![computer.public], false),
        )
    }

    #[test]
    fn pairs_when_both_people_agree() {
        let (computer, phone) = keys();
        let mut pair = Pair::new(
            Session::new(Role::Computer, &computer, Vec::new(), true),
            Session::new(Role::Phone, &phone, Vec::new(), false),
        );
        let code = Pair::code(&pair.computer_events).expect("컴퓨터 숫자");
        assert_eq!(Pair::code(&pair.phone_events), Some(code.clone()));
        assert_eq!(code.len(), 6);
        pair.computer.confirm(true).unwrap();
        pair.pump().unwrap();
        assert!(!pair.computer.is_ready(), "한쪽만 맞다고 함");
        pair.phone.confirm(true).unwrap();
        pair.pump().unwrap();
        assert!(
            pair.computer_events
                .contains(&Event::Paired { peer: phone.public })
        );
        assert!(pair.phone_events.contains(&Event::Paired {
            peer: computer.public
        }));
        assert!(pair.computer.is_ready() && pair.phone.is_ready());
    }

    #[test]
    fn codes_differ_between_connections() {
        let (computer, phone) = keys();
        let codes: Vec<_> = (0..3)
            .map(|_| {
                let pair = Pair::new(
                    Session::new(Role::Computer, &computer, Vec::new(), true),
                    Session::new(Role::Phone, &phone, Vec::new(), false),
                );
                Pair::code(&pair.phone_events).unwrap()
            })
            .collect();
        assert!(codes[0] != codes[1] || codes[1] != codes[2]);
    }

    #[test]
    fn either_side_can_reject() {
        for computer_says in [false, true] {
            let (computer, phone) = keys();
            let mut pair = Pair::new(
                Session::new(Role::Computer, &computer, Vec::new(), true),
                Session::new(Role::Phone, &phone, Vec::new(), false),
            );
            let (yes, no) = if computer_says {
                (&mut pair.phone, &mut pair.computer)
            } else {
                (&mut pair.computer, &mut pair.phone)
            };
            yes.confirm(true).unwrap();
            assert_eq!(no.confirm(false), Err(SessionError::Rejected));
            let error = pair.pump().unwrap_err();
            assert_eq!(error.1, SessionError::Rejected);
            assert!(
                !pair
                    .computer_events
                    .iter()
                    .any(|e| matches!(e, Event::Paired { .. }))
            );
            assert!(
                !pair
                    .phone_events
                    .iter()
                    .any(|e| matches!(e, Event::Paired { .. }))
            );
        }
    }

    #[test]
    fn known_keys_connect_without_a_code() {
        let (computer, phone) = keys();
        let mut pair = known(&computer, &phone);
        assert_eq!(pair.computer_events, [Event::Ready { peer: phone.public }]);
        assert_eq!(
            pair.phone_events,
            [Event::Ready {
                peer: computer.public
            }]
        );
        let hello = Body::Hello(Hello {
            computer_name: "맥".into(),
        });
        pair.computer.send(hello.clone()).unwrap();
        pair.computer.send(Body::Ping(Default::default())).unwrap();
        pair.pump().unwrap();
        assert_eq!(pair.phone_events.last(), Some(&Event::Message(hello)));
    }

    #[test]
    fn strangers_are_turned_away() {
        let (computer, phone) = keys();
        let stranger = Identity::generate();
        let mut pair = Pair::new(
            Session::new(Role::Computer, &computer, vec![phone.public], false),
            Session::new(Role::Phone, &phone, vec![stranger.public], false),
        );
        assert_eq!(pair.pump().unwrap_err().1, SessionError::NotPaired);
        assert!(pair.phone.feed(&[]).is_err() || pair.computer.feed(&[]).is_err());

        let mut pair = Pair::new(
            Session::new(Role::Computer, &computer, vec![stranger.public], false),
            Session::new(Role::Phone, &phone, vec![computer.public], false),
        );
        assert_eq!(pair.pump().unwrap_err().1, SessionError::NotPaired);
    }

    #[test]
    fn the_phone_never_opens_pairing() {
        let (computer, phone) = keys();
        let mut pair = Pair::new(
            Session::new(Role::Computer, &computer, Vec::new(), false),
            Session::new(Role::Phone, &phone, Vec::new(), true),
        );
        assert_eq!(pair.pump().unwrap_err().1, SessionError::NotPaired);
        assert_eq!(Pair::code(&pair.phone_events), None);
    }

    #[test]
    fn a_phone_refusing_pairing_shows_no_code() {
        let (computer, phone) = keys();
        let mut pair = Pair::new(
            Session::new(Role::Computer, &computer, Vec::new(), true),
            Session::new(Role::Phone, &phone, Vec::new(), false).refusing_pairing(),
        );
        assert_eq!(
            pair.pump().unwrap_err(),
            (Role::Phone, SessionError::NotPaired)
        );
        assert_eq!(Pair::code(&pair.phone_events), None);
        assert_eq!(Pair::code(&pair.computer_events), None);
    }

    #[test]
    fn a_phone_refusing_pairing_still_takes_a_known_computer() {
        let (computer, phone) = keys();
        let mut pair = Pair::new(
            Session::new(Role::Computer, &computer, vec![phone.public], false),
            Session::new(Role::Phone, &phone, vec![computer.public], false).refusing_pairing(),
        );
        pair.pump().unwrap();
        assert_eq!(
            pair.phone_events,
            [Event::Ready {
                peer: computer.public
            }]
        );
    }

    #[test]
    fn rejects_another_major_version() {
        let (_, phone) = keys();
        let mut session = Session::new(Role::Phone, &phone, Vec::new(), false);
        assert_eq!(
            session.feed(&[b'S', b'D', VERSION[0] + 1, 0]),
            Err(SessionError::Version {
                mine: VERSION[0],
                theirs: VERSION[0] + 1
            })
        );
        let mut session = Session::new(Role::Phone, &phone, Vec::new(), false);
        assert!(matches!(
            session.feed(b"GET /"),
            Err(SessionError::Broken(_))
        ));
    }

    #[test]
    fn a_minor_version_is_fine_but_cannot_be_changed_on_the_way() {
        let (computer, phone) = keys();
        let mut c = Session::new(Role::Computer, &computer, vec![phone.public], false);
        let mut p = Session::new(Role::Phone, &phone, vec![computer.public], false);
        let mut bytes = c.take_output();
        bytes[3] += 1;
        assert_eq!(p.feed(&bytes), Ok(()));
        // 컴퓨터는 바뀌기 전 머리말로 핸드셰이크를 하므로 폰의 두 번째 메시지를 못 푼다.
        c.feed(&p.take_output()).unwrap();
        p.feed(&c.take_output()).unwrap();
        assert_eq!(
            c.feed(&p.take_output()),
            Err(SessionError::Broken("핸드셰이크 실패"))
        );
    }

    #[test]
    fn pings_flow_while_people_look_at_the_code() {
        let (computer, phone) = keys();
        let mut pair = Pair::new(
            Session::new(Role::Computer, &computer, Vec::new(), true),
            Session::new(Role::Phone, &phone, Vec::new(), false),
        );
        assert!(Pair::code(&pair.phone_events).is_some());
        pair.computer.write(Body::Ping(Default::default())).unwrap();
        pair.pump().unwrap();
        pair.computer.confirm(true).unwrap();
        pair.phone.confirm(true).unwrap();
        pair.pump().unwrap();
        assert!(pair.computer.is_ready() && pair.phone.is_ready());
    }

    #[test]
    fn works_one_byte_at_a_time() {
        let (computer, phone) = keys();
        let mut a = Session::new(Role::Computer, &computer, vec![phone.public], false);
        let mut b = Session::new(Role::Phone, &phone, vec![computer.public], false);
        for _ in 0..4 {
            for byte in a.take_output() {
                b.feed(&[byte]).unwrap();
            }
            for byte in b.take_output() {
                a.feed(&[byte]).unwrap();
            }
        }
        assert!(a.is_ready() && b.is_ready());
    }

    #[test]
    fn a_flipped_byte_breaks_the_connection() {
        let (computer, phone) = keys();
        let mut pair = known(&computer, &phone);
        pair.computer.send(Body::Ping(Default::default())).unwrap();
        let mut bytes = pair.computer.take_output();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        assert_eq!(
            pair.phone.feed(&bytes),
            Err(SessionError::Broken("복호화 실패"))
        );
        assert!(pair.phone.feed(&[]).is_err());
    }

    #[test]
    fn a_broken_promise_is_caught() {
        let (computer, phone) = keys();
        let mut c = Session::new(Role::Computer, &computer, Vec::new(), true);
        let mut p = Session::new(Role::Phone, &phone, Vec::new(), false);
        while !matches!(p.stage, Stage::AwaitReveal { .. }) {
            p.feed(&c.take_output()).unwrap();
            c.feed(&p.take_output()).unwrap();
        }
        // 폰이 받은 약속을 다른 값으로 바꿔, 공개된 난수와 맞지 않게 한다.
        p.stage = Stage::AwaitReveal {
            commit: commit(&[7; NONCE_LEN]),
        };
        assert_eq!(
            p.feed(&c.take_output()),
            Err(SessionError::Broken("등록 확인값이 맞지 않음"))
        );
    }

    #[test]
    fn nothing_but_a_ready_session_sends() {
        let (computer, phone) = keys();
        let mut session = Session::new(Role::Computer, &computer, vec![phone.public], false);
        assert!(session.send(Body::Ping(Default::default())).is_err());
    }
}
