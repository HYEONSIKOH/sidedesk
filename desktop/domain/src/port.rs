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

pub trait Keys: Send + Sync {
    fn identity(&self) -> Result<Identity, PortError>;
    fn trusted(&self) -> Result<Vec<[u8; KEY_LEN]>, PortError>;
    fn trust(&self, key: [u8; KEY_LEN]) -> Result<(), PortError>;
}

/// 짝 맺기 숫자를 사람에게 보이고 답을 받는다. 답은 나중에 채널로 온다.
pub trait Prompt: Send + Sync {
    fn ask(&self, phone: &str, code: &str) -> Receiver<bool>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Note {
    Connected,
    Paired,
    /// 접속하지 못했거나 끊김. 다시 시도한다.
    Lost(String),
    /// 다시 시도해도 안 되는 이유. 폰을 다시 꽂을 때까지 멈춘다.
    Refused(String),
}

pub trait Report: Send + Sync {
    fn note(&self, phone: &str, note: Note);
}
