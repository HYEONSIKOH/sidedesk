use std::collections::HashSet;
use std::sync::Mutex;
use std::sync::mpsc::Receiver;

use sidedesk_core::{Identity, KEY_LEN};

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct PortError(pub String);

impl PortError {
    pub fn new(reason: impl Into<String>) -> Self {
        Self(reason.into())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Phone {
    pub id: String,
    /// adb 가 알려 준 기기 이름
    pub model: Option<String>,
    /// 지금 접속할 수 없으면 그 이유
    pub blocked: Option<String>,
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

    /// 붙은 폰 목록이 바뀌었다.
    fn phones(&self, _phones: &[Phone]) {}
}
