use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Mutex;
use std::sync::mpsc::Receiver;

use sidedesk_core::{BEACON_LEN, Identity, KEY_LEN};

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct PortError(pub String);

impl PortError {
    pub fn new(reason: impl Into<String>) -> Self {
        Self(reason.into())
    }
}

/// 기기와 이어지는 길
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Link {
    Usb,
    Ble,
}

impl fmt::Display for Link {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Usb => "USB",
            Self::Ble => "블루투스",
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Phone {
    /// 링크 안에서 기기를 가리키는 값. USB 는 adb 일련번호, 블루투스는 `ble:` + 맥이 정한 식별자.
    pub id: String,
    /// 기기가 알려 준 이름
    pub model: Option<String>,
    /// 지금 접속할 수 없으면 그 이유
    pub blocked: Option<String>,
    pub link: Link,
    /// 블루투스 알림 신호 지문. USB 는 없다.
    pub beacon: Option<[u8; BEACON_LEN]>,
}

impl Phone {
    pub fn usb(id: &str, model: Option<String>, blocked: Option<String>) -> Self {
        Self {
            id: id.to_owned(),
            model,
            blocked,
            link: Link::Usb,
            beacon: None,
        }
    }
}

pub enum Received {
    Bytes(usize),
    /// 잠깐 기다렸는데 온 것이 없음
    Nothing,
    Closed,
}

/// 폰과 이어진 통로 하나. 읽기는 짧게 기다리고 돌아온다.
pub trait Conn: Send {
    fn receive(&mut self, buffer: &mut [u8]) -> Result<Received, PortError>;
    fn send(&mut self, bytes: &[u8]) -> Result<(), PortError>;
}

pub trait PhoneLinks: Send + Sync {
    fn link(&self) -> Link;
    fn attached(&self) -> Result<Vec<Phone>, PortError>;
    /// 붙은 폰 목록이 바뀔 때마다 알린다. 통로가 끊기면 돌아온다.
    fn watch(&self, changed: &mut dyn FnMut(Vec<Phone>)) -> Result<(), PortError>;
    fn connect(&self, id: &str) -> Result<Box<dyn Conn>, PortError>;
}

pub trait ComputerName: Send + Sync {
    fn read(&self) -> Result<String, PortError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paired {
    pub key: [u8; KEY_LEN],
    pub name: String,
    /// 등록할 때 adb 가 알려 준 일련번호. 옛 기록은 비어 있다.
    pub serial: String,
}

pub trait Keys: Send + Sync {
    fn identity(&self) -> Result<Identity, PortError>;
    fn trusted(&self) -> Result<Vec<Paired>, PortError>;
    /// 새 키는 끝에 더하고, 이미 있는 키는 그 자리에서 이름과 일련번호를 바꾼다.
    fn trust(&self, key: [u8; KEY_LEN], name: &str, serial: &str) -> Result<(), PortError>;
    /// 키가 남아 있을 때만 이름과 일련번호를 바꾼다. 해제된 키를 되살리지 않는다.
    fn update(&self, key: [u8; KEY_LEN], name: &str, serial: &str) -> Result<(), PortError>;
    fn forget(&self, key: [u8; KEY_LEN]) -> Result<(), PortError>;
}

/// 사람이 짝 맺기를 누른 폰. 그 폰의 다음 접속은 짝 맺기로 한다.
#[derive(Default)]
pub struct PairRequests(Mutex<HashSet<String>>);

impl PairRequests {
    pub fn ask(&self, id: &str) {
        self.lock().insert(id.to_owned());
    }

    pub fn is_asked(&self, id: &str) -> bool {
        self.lock().contains(id)
    }

    /// 요청이 있었으면 지우고 true
    pub fn take(&self, id: &str) -> bool {
        self.lock().remove(id)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashSet<String>> {
        self.0.lock().unwrap_or_else(|lock| lock.into_inner())
    }
}

/// 짝 맺기 숫자를 사람에게 보이고 답을 받는다. 답은 나중에 채널로 온다.
pub trait Prompt: Send + Sync {
    fn ask(&self, phone: &str, code: &str) -> Receiver<bool>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Note {
    Connected {
        key: [u8; KEY_LEN],
    },
    Paired {
        key: [u8; KEY_LEN],
    },
    /// 접속하지 못했거나 끊김. 다시 시도한다.
    Lost(String),
    /// 다시 시도해도 안 되는 이유. 폰을 다시 꽂을 때까지 멈춘다.
    Refused(String),
}

pub trait Report: Send + Sync {
    fn note(&self, phone: &str, note: Note);

    /// 한 링크의 기기 목록이 바뀌었다. 다른 링크의 목록은 건드리지 않는다.
    fn phones(&self, _link: Link, _phones: &[Phone]) {}

    /// 링크 자체의 문제(adb 없음, 블루투스 꺼짐, 권한 없음). None 이면 문제가 풀렸다.
    fn link(&self, _link: Link, _problem: Option<String>) {}
}

/// 지금 붙어 있는 기기 키와 그 길, 그 연결의 id. 한 기기에는 연결 하나만 붙인다(USB 우선).
#[derive(Default)]
pub struct Active(Mutex<HashMap<[u8; KEY_LEN], (Link, String)>>);

impl Active {
    /// 이 연결로 붙어도 되면 자리를 잡고 true.
    /// 블루투스는 USB 가 있거나, 같은 기기가 다른 블루투스 id(주소가 바뀐 경우)로 붙어 있으면 진다.
    pub fn claim(&self, key: [u8; KEY_LEN], link: Link, id: &str) -> bool {
        let mut held = self.lock();
        let lose = match (held.get(&key), link) {
            (Some((Link::Usb, _)), Link::Ble) => true,
            (Some((Link::Ble, other)), Link::Ble) => other != id,
            _ => false,
        };
        if !lose {
            held.insert(key, (link, id.to_owned()));
        }
        !lose
    }

    /// 이 연결이 잡은 자리만 놓는다.
    pub fn release(&self, key: [u8; KEY_LEN], id: &str) {
        let mut held = self.lock();
        if held.get(&key).is_some_and(|(_, holder)| holder == id) {
            held.remove(&key);
        }
    }

    pub fn holds(&self, key: [u8; KEY_LEN], link: Link) -> bool {
        self.lock().get(&key).is_some_and(|(held, _)| *held == link)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<[u8; KEY_LEN], (Link, String)>> {
        self.0.lock().unwrap_or_else(|lock| lock.into_inner())
    }
}
