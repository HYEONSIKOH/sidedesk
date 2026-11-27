use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use sidedesk_core::{Body, Event, Hello, KEY_LEN, Role, Session, SessionError};

use crate::hub::{Hub, Subscriber};
use crate::port::{
    Active, ComputerName, Conn, Keys, Link, Note, PairRequests, Phone, PortError, Prompt, Received,
    Report,
};

#[derive(Clone, Copy, Debug)]
pub struct Timing {
    pub ping: Duration,
    /// 이만큼 아무것도 안 오면 끊긴 것으로 본다.
    pub silence: Duration,
    /// 짝 맺기 숫자를 확인하기까지 기다리는 시간
    pub answer: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            ping: Duration::from_secs(5),
            silence: Duration::from_secs(15),
            answer: Duration::from_secs(60),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Ended {
    Closed,
    Stopped,
    /// 짝을 맺었다. 짝 맺기로 붙었을 때만 여기서 끝난다.
    Paired,
    /// 같은 기기가 USB 로 붙어 있어 블루투스가 스스로 물러났다. 알릴 것이 없다.
    Yielded,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum ServeError {
    #[error(transparent)]
    Session(#[from] SessionError),
    #[error(transparent)]
    Port(#[from] PortError),
    #[error("기기가 응답하지 않습니다")]
    Silent,
    #[error("등록 번호를 확인하지 않음")]
    NoAnswer,
    #[error("등록을 해제함")]
    Forgotten,
    #[error("다른 컴퓨터가 이 기기를 쓰고 있거나 USB 로 붙어 있습니다")]
    Busy,
}

impl ServeError {
    /// 다시 시도해도 같은 결과가 나오는 오류
    pub fn is_final(&self) -> bool {
        matches!(
            self,
            Self::Session(
                SessionError::NotPaired | SessionError::Version { .. } | SessionError::Rejected
            ) | Self::Forgotten
        )
    }
}

pub struct Context<'a> {
    pub name: &'a dyn ComputerName,
    pub keys: &'a dyn Keys,
    pub prompt: &'a dyn Prompt,
    pub report: &'a dyn Report,
    pub requests: &'a PairRequests,
    pub active: &'a Active,
    pub hub: &'a Hub,
    pub timing: Timing,
}

/// 잡은 자리를 연결이 끝날 때 놓는다.
struct Hold<'a> {
    active: &'a Active,
    key: [u8; KEY_LEN],
    id: &'a str,
}

impl Drop for Hold<'_> {
    fn drop(&mut self) {
        self.active.release(self.key, self.id);
    }
}

/// 폰 하나와 연결을 맺고, 끊길 때까지 살아 있게 한다.
/// 블루투스에서 기기가 받아들이기 전에(첫 Pong 전에) 끊기면, 보내기 오류든 닫힘이든 `Busy` 로 본다.
pub fn serve(
    phone: &Phone,
    conn: &mut dyn Conn,
    context: &Context,
    pairing: bool,
    stop: &AtomicBool,
) -> Result<Ended, ServeError> {
    let confirming = std::cell::Cell::new(false);
    match run(phone, conn, context, pairing, stop, &confirming) {
        Err(ServeError::Port(_)) if confirming.get() => Err(ServeError::Busy),
        other => other,
    }
}

fn run(
    phone: &Phone,
    conn: &mut dyn Conn,
    context: &Context,
    pairing: bool,
    stop: &AtomicBool,
    confirming: &std::cell::Cell<bool>,
) -> Result<Ended, ServeError> {
    let identity = context.keys.identity()?;
    let trusted = context
        .keys
        .trusted()?
        .into_iter()
        .map(|paired| paired.key)
        .collect();
    let mut session = Session::new(Role::Computer, &identity, trusted, pairing);
    let id = phone.id.as_str();
    let mut peer = None;
    let mut _hold = None;
    let mut buffer = vec![0; 16 * 1024];
    let mut heard = Instant::now();
    let mut next_ping = Instant::now() + context.timing.ping;
    let mut answer: Option<Receiver<bool>> = None;
    // 숫자가 뜬 뒤 양쪽 답을 기다리는 마감. 컴퓨터가 먼저 답해도 폰의 답을 기다리며 남는다.
    let mut deadline: Option<Instant> = None;
    let mut subscriber: Option<Subscriber> = None;
    loop {
        if let Some(subscriber) = &subscriber {
            for body in subscriber.take() {
                // 그림은 커서 다른 값과 따로 보낸다(블루투스 한 번의 쓰기를 작게).
                let big = matches!(body, Body::Artwork(_));
                if big {
                    let before = session.take_output();
                    if !before.is_empty() {
                        conn.send(&before)?;
                    }
                }
                session.send(body)?;
                if big {
                    conn.send(&session.take_output())?;
                }
            }
        }
        let output = session.take_output();
        if !output.is_empty() {
            conn.send(&output)?;
        }
        while let Some(event) = session.next_event() {
            match event {
                Event::PairingCode(code) => {
                    answer = Some(context.prompt.ask(id, &code));
                    deadline = Some(Instant::now() + context.timing.answer);
                }
                Event::Paired { peer } => {
                    let told = session.peer_name().map(str::to_owned);
                    let name = told.as_deref().or(phone.model.as_deref()).unwrap_or(id);
                    context.keys.trust(peer, name, id)?;
                    context.report.note(id, Note::Paired { key: peer });
                }
                Event::Ready { peer: key } => {
                    // 같은 기기에 다른 길이 이미 있으면 아무것도 알리지 않고 물러난다.
                    if !context.active.claim(key, phone.link, id) {
                        return Ok(Ended::Yielded);
                    }
                    _hold = Some(Hold {
                        active: context.active,
                        key,
                        id,
                    });
                    peer = Some(key);
                    subscriber = Some(context.hub.join_over(phone.link));
                    let told = session.peer_name().map(str::to_owned);
                    fill_record(context, phone, key, told.as_deref());
                    let computer_name = context.name.read()?;
                    session.send(Body::Hello(Hello { computer_name }))?;
                    match phone.link {
                        Link::Usb => context.report.note(id, Note::Connected { key }),
                        Link::Ble => {
                            session.ping()?;
                            next_ping = Instant::now() + context.timing.ping;
                            confirming.set(true);
                        }
                    }
                }
                Event::Message(Body::Subscribe(subscribe)) => {
                    if let Some(subscriber) = &subscriber {
                        subscriber.set(&subscribe);
                    }
                }
                Event::Message(Body::Location(location)) => {
                    if let Some(subscriber) = &subscriber {
                        subscriber.place(&location);
                    }
                }
                Event::Message(Body::MusicCommand(command)) => {
                    if let Some(subscriber) = &subscriber {
                        if !is_trusted(context, peer)? {
                            return Err(ServeError::Forgotten);
                        }
                        subscriber.music(&command);
                    }
                }
                Event::Message(Body::RunShortcut(command)) => {
                    // 키 보내기, 잠금처럼 넓은 일을 하므로 명령마다 등록이 남아 있는지 본다.
                    if let Some(subscriber) = &subscriber {
                        if !is_trusted(context, peer)? {
                            return Err(ServeError::Forgotten);
                        }
                        subscriber.run_shortcut(&command);
                    }
                }
                Event::Message(Body::KillProcess(command)) if subscriber.is_some() => {
                    // Ping 주기를 기다리지 않고, 명령마다 등록이 남아 있는지 본다.
                    if !is_trusted(context, peer)? {
                        return Err(ServeError::Forgotten);
                    }
                    let result = context.hub.kill(&command);
                    session.send(Body::CommandResult(result))?;
                }
                Event::Message(_) => {}
            }
        }
        if pairing && session.is_ready() {
            conn.send(&session.take_output())?;
            return Ok(Ended::Paired);
        }
        if stop.load(Ordering::Relaxed) {
            return Ok(Ended::Stopped);
        }
        if session.is_ready() {
            deadline = None;
        }
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Err(ServeError::NoAnswer);
        }
        if let Some(reply) = &answer {
            match reply.try_recv() {
                Ok(same) => {
                    answer = None;
                    if let Err(error) = session.confirm(same) {
                        let _ = conn.send(&session.take_output());
                        return Err(error.into());
                    }
                    continue;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => return Err(ServeError::NoAnswer),
            }
        }
        if Instant::now() >= next_ping {
            // 그 사이 짝을 끊었으면 더 붙어 있지 않는다.
            if !is_trusted(context, peer)? {
                return Err(ServeError::Forgotten);
            }
            session.ping()?;
            next_ping = Instant::now() + context.timing.ping;
            continue;
        }
        match conn.receive(&mut buffer)? {
            Received::Bytes(count) => {
                heard = Instant::now();
                session.feed(&buffer[..count])?;
                if confirming.get()
                    && let Some(key) = peer
                {
                    confirming.set(false);
                    context.report.note(id, Note::Connected { key });
                }
            }
            Received::Nothing if heard.elapsed() > context.timing.silence => {
                return Err(ServeError::Silent);
            }
            Received::Nothing => {}
            Received::Closed if confirming.get() => return Err(ServeError::Busy),
            Received::Closed => return Ok(Ended::Closed),
        }
    }
}

/// 아직 상대를 모르면(준비 전) 막지 않는다.
fn is_trusted(context: &Context, peer: Option<[u8; KEY_LEN]>) -> Result<bool, ServeError> {
    let Some(key) = peer else {
        return Ok(true);
    };
    Ok(context
        .keys
        .trusted()?
        .iter()
        .any(|paired| paired.key == key))
}

/// 이름이나 일련번호 없이 저장된 옛 기록을 채운다. 실패해도 연결은 잇는다.
/// 일련번호는 USB 로 붙었을 때만 채운다(블루투스 식별자는 일련번호가 아니다).
/// 폰이 기종 이름을 알려 주면, 저장된 이름이 비었거나 자동으로 붙인 이름(USB 목록의 이름, 일련번호)일 때만 바꾼다.
fn fill_record(context: &Context, phone: &Phone, key: [u8; KEY_LEN], told: Option<&str>) {
    let Ok(trusted) = context.keys.trusted() else {
        return;
    };
    let Some(saved) = trusted.into_iter().find(|p| p.key == key) else {
        return;
    };
    let id = phone.id.as_str();
    let usb = phone.link == Link::Usb;
    let automatic = saved.name.is_empty()
        || phone.model.as_deref() == Some(saved.name.as_str())
        || saved.name == id
        || saved.name == saved.serial;
    let name = match (told, automatic) {
        (Some(told), true) => told,
        _ if saved.name.is_empty() => phone.model.as_deref().unwrap_or(if usb { id } else { "" }),
        _ => saved.name.as_str(),
    };
    let serial = match (saved.serial.as_str(), usb) {
        ("", true) => id,
        (serial, _) => serial,
    };
    if name != saved.name || serial != saved.serial {
        let _ = context.keys.update(key, name, serial);
    }
}
