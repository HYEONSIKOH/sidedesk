use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Mutex;
use std::sync::mpsc::Receiver;

use sidedesk_core::{AiTool, BEACON_LEN, Forge, Identity, KEY_LEN, Performance, Process, Weather};

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct PortError(pub String);

impl PortError {
    pub fn new(reason: impl Into<String>) -> Self {
        Self(reason.into())
    }
}

/// 기기와 이어지는 길
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
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

/// 기기 목록을 내는 곳. 같은 길에 출처가 여럿일 수 있다(USB 는 adb 와 usbmux).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Source {
    pub link: Link,
    pub name: &'static str,
}

impl Source {
    pub const fn new(link: Link, name: &'static str) -> Self {
        Self { link, name }
    }

    /// 그 길에 출처가 하나뿐일 때의 이름
    pub const fn of(link: Link) -> Self {
        match link {
            Link::Usb => Self::new(link, "usb"),
            Link::Ble => Self::new(link, "ble"),
        }
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
    /// 목록과 문제를 알릴 때 쓰는 출처. 같은 길의 다른 출처와 이름이 달라야 한다.
    fn source(&self) -> Source {
        Source::of(self.link())
    }
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

    /// 한 출처의 기기 목록이 바뀌었다. 다른 출처의 목록은 건드리지 않는다.
    fn phones(&self, _source: Source, _phones: &[Phone]) {}

    /// 출처 자체의 문제(adb 없음, 블루투스 꺼짐, 권한 없음). None 이면 문제가 풀렸다.
    fn link(&self, _source: Source, _problem: Option<String>) {}
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

/// 성능 수집기. 앞 값과의 차이로 비율과 속도를 낸다.
pub trait Meter: Send + Sync {
    fn performance(&self) -> Result<Performance, PortError>;
}

pub trait ProcessTable: Send + Sync {
    /// 읽을 수 있는 프로세스 전부. CPU 는 앞 호출과의 차이로 낸다.
    fn processes(&self) -> Result<Vec<Process>, PortError>;
    /// 번호와 시작 시각이 둘 다 맞을 때만 끝낸다. 실패하면 사람이 읽을 까닭을 돌려준다.
    fn kill(&self, pid: u32, started_at: u64) -> Result<(), PortError>;
}

/// 대략적인 위치. 위도, 경도 x10.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Place {
    pub latitude: i16,
    pub longitude: i16,
}

/// 날씨 받는 곳. 오류 문장에 좌표, 주소, 키를 넣지 않는다.
pub trait WeatherSource: Send + Sync {
    fn fetch(&self, place: Place) -> Result<Weather, PortError>;
}

/// 인증키 같은 비밀 값. 찍어도 보이지 않고, 놓을 때 지운다.
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl Clone for Secret {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(..)")
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        // SAFETY: 0 바이트는 올바른 UTF-8 이다.
        unsafe { self.0.as_bytes_mut() }.fill(0);
    }
}

/// 운영체제의 비밀 보관소(맥 키체인, 윈도우 자격 증명 관리자)
pub trait Secrets: Send + Sync {
    fn kma_key(&self) -> Result<Option<Secret>, PortError>;
    /// 값을 꺼내지 않고 있는지만 본다.
    fn has_kma_key(&self) -> Result<bool, PortError>;
    fn set_kma_key(&self, key: &Secret) -> Result<(), PortError>;
    fn clear_kma_key(&self) -> Result<(), PortError>;
}

/// 사람이 고른 날씨 제공처
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Choice {
    #[default]
    OpenMeteo,
    Kma,
}

/// 기상청 인증키를 한 번 불러 본 결과
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyCheck {
    Good,
    /// 키는 맞지만 오늘 한도를 넘음
    Limit,
    Wrong,
    Offline,
    Unknown,
}

/// 설정 창이 쓰는 날씨 설정
pub trait WeatherSetup: Send + Sync {
    /// 이 운영체제에서 날씨를 받는지
    fn supported(&self) -> bool;
    fn choice(&self) -> Choice;
    fn set_choice(&self, choice: Choice) -> Result<(), PortError>;
    /// 시작 때 한 번 보고 들고 있는 값
    fn has_key(&self) -> bool;
    /// 넣은 키의 앞 몇 글자. 설정 창이 나머지를 가려 보인다
    fn key_hint(&self) -> Option<String> {
        None
    }
    fn check_key(&self, key: &Secret) -> KeyCheck;
    fn save_key(&self, key: &Secret) -> Result<(), PortError>;
    fn clear_key(&self) -> Result<(), PortError>;
}

/// 기록에서 읽은 세션의 단계. 상태 이름(끝남 등)은 domain 이 정한다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Working,
    /// 도구 허락이나 사람의 답을 기다림
    Waiting,
    /// 답을 마침(10초 지나면 끝남)
    TurnEnded,
    /// 사람이 멈췄거나 작업 중에 닫힘. 끝남으로 보이되 알리지 않는다.
    Stopped,
    Unknown,
}

/// 기록에서 읽은 한 세션의 사실. 대화 내용은 담지 않는다.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionFacts {
    /// 도구와 세션 번호. 밖으로 보내지 않는다.
    pub key: String,
    pub tool: AiTool,
    /// 줄 바꿈을 지우고 60글자로 자른 제목
    pub title: String,
    /// 작업 폴더의 마지막 이름
    pub project: String,
    pub open: bool,
    pub phase: Phase,
    /// 마지막 기록 시각
    pub last_activity: std::time::SystemTime,
}

/// AI 세션 기록 읽기. 오류 문장에 경로나 기록 내용을 넣지 않는다.
pub trait SessionLog: Send + Sync {
    /// 바뀐 파일만 읽고 지금 아는 세션 전부를 돌려준다.
    fn poll(&self) -> Result<Vec<SessionFacts>, PortError>;
}

/// GitHub, GitLab 수. 실패도 상태로 돌려준다.
pub trait Forges: Send + Sync {
    fn counts(&self) -> Forge;
    /// 돌고 있는 외부 프로그램을 끝낸다(프로그램을 끌 때).
    fn stop(&self) {}
}
