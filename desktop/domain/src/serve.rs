use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use sidedesk_core::{Body, Event, Hello, Role, Session, SessionError};

use crate::port::{ComputerName, Conn, Keys, Note, PortError, Prompt, Received, Report};

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
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum ServeError {
    #[error(transparent)]
    Session(#[from] SessionError),
    #[error(transparent)]
    Port(#[from] PortError),
    #[error("폰이 응답하지 않음")]
    Silent,
    #[error("짝 맺기 숫자를 확인하지 않음")]
    NoAnswer,
}

impl ServeError {
    /// 다시 시도해도 같은 결과가 나오는 오류
    pub fn is_final(&self) -> bool {
        matches!(
            self,
            Self::Session(
                SessionError::NotPaired | SessionError::Version { .. } | SessionError::Rejected
            )
        )
    }
}

pub struct Context<'a> {
    pub name: &'a dyn ComputerName,
    pub keys: &'a dyn Keys,
    pub prompt: &'a dyn Prompt,
    pub report: &'a dyn Report,
    pub timing: Timing,
}

/// 폰 하나와 연결을 맺고, 끊길 때까지 살아 있게 한다.
pub fn serve(
    phone: &str,
    conn: &mut dyn Conn,
    context: &Context,
    pairing: bool,
    stop: &AtomicBool,
) -> Result<Ended, ServeError> {
    let identity = context.keys.identity()?;
    let mut session = Session::new(Role::Computer, &identity, context.keys.trusted()?, pairing);
    let mut buffer = vec![0; 16 * 1024];
    let mut heard = Instant::now();
    let mut next_ping = Instant::now() + context.timing.ping;
    let mut answer: Option<Receiver<bool>> = None;
    // 숫자가 뜬 뒤 양쪽 답을 기다리는 마감. 컴퓨터가 먼저 답해도 폰의 답을 기다리며 남는다.
    let mut deadline: Option<Instant> = None;
    loop {
        let output = session.take_output();
        if !output.is_empty() {
            conn.send(&output)?;
        }
        while let Some(event) = session.next_event() {
            match event {
                Event::PairingCode(code) => {
                    answer = Some(context.prompt.ask(phone, &code));
                    deadline = Some(Instant::now() + context.timing.answer);
                }
                Event::Paired { peer } => {
                    context.keys.trust(peer)?;
                    context.report.note(phone, Note::Paired);
                }
                Event::Ready { .. } => {
                    let computer_name = context.name.read()?;
                    session.send(Body::Hello(Hello { computer_name }))?;
                    context.report.note(phone, Note::Connected);
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
            session.ping()?;
            next_ping = Instant::now() + context.timing.ping;
            continue;
        }
        match conn.receive(&mut buffer)? {
            Received::Bytes(count) => {
                heard = Instant::now();
                session.feed(&buffer[..count])?;
            }
            Received::Nothing if heard.elapsed() > context.timing.silence => {
                return Err(ServeError::Silent);
            }
            Received::Nothing => {}
            Received::Closed => return Ok(Ended::Closed),
        }
    }
}
