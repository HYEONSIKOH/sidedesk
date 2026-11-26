//! 폰 앱(Kotlin, Swift)이 쓰는 연결.

use std::sync::{Arc, Mutex, MutexGuard};

use crate::{
    Body, CommandResult, Event, Identity, KEY_LEN, KillProcess, Performance, Process, Processes,
    Role, Session, SessionError, Subscribe,
};

#[derive(uniffi::Record)]
pub struct KeyPair {
    pub private_key: Vec<u8>,
    pub public_key: Vec<u8>,
}

#[uniffi::export]
pub fn generate_key_pair() -> KeyPair {
    let identity = Identity::generate();
    KeyPair {
        private_key: identity.private.to_vec(),
        public_key: identity.public.to_vec(),
    }
}

#[derive(Debug, PartialEq, uniffi::Enum)]
pub enum PhoneEvent {
    PairingCode {
        code: String,
    },
    Paired {
        computer_key: Vec<u8>,
    },
    Ready {
        computer_key: Vec<u8>,
    },
    ComputerName {
        name: String,
    },
    Performance {
        value: PerformanceInfo,
    },
    Processes {
        items: Vec<ProcessInfo>,
    },
    CommandResult {
        request_id: u64,
        ok: bool,
        reason: String,
    },
}

/// 폰에 보이는 자료. 시계는 폰이 혼자 그리므로 없다.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum View {
    Performance,
    Processes,
}

impl From<View> for crate::View {
    fn from(view: View) -> Self {
        match view {
            View::Performance => Self::Performance,
            View::Processes => Self::Processes,
        }
    }
}

/// 비율은 0~100, 네트워크는 초당 바이트.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PerformanceInfo {
    pub cpu: f32,
    pub gpu: Option<f32>,
    pub memory: f32,
    pub memory_used: u64,
    pub memory_total: u64,
    pub disk: f32,
    pub net_down: u64,
    pub net_up: u64,
}

impl From<Performance> for PerformanceInfo {
    fn from(p: Performance) -> Self {
        Self {
            cpu: p.cpu,
            gpu: p.gpu,
            memory: p.memory,
            memory_used: p.memory_used,
            memory_total: p.memory_total,
            disk: p.disk,
            net_down: p.net_down,
            net_up: p.net_up,
        }
    }
}

/// cpu 는 코어 하나가 100, memory 는 바이트, started_at 은 유닉스 밀리초.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub cpu: f32,
    pub memory: u64,
    pub started_at: u64,
}

impl From<Process> for ProcessInfo {
    fn from(p: Process) -> Self {
        Self {
            pid: p.pid,
            name: p.name,
            cpu: p.cpu,
            memory: p.memory,
            started_at: p.started_at,
        }
    }
}

/// 명령 번호. 같은 명령을 다시 보낼 때는 같은 번호를 쓴다.
#[uniffi::export]
pub fn new_request_id() -> u64 {
    let mut bytes = [0; 8];
    getrandom::fill(&mut bytes).expect("난수를 얻지 못함");
    u64::from_le_bytes(bytes)
}

#[derive(Debug, PartialEq, thiserror::Error, uniffi::Error)]
pub enum LinkError {
    #[error("버전이 다름")]
    Version { mine: u8, theirs: u8 },
    #[error("등록되지 않은 컴퓨터")]
    NotPaired,
    #[error("등록을 취소함")]
    Rejected,
    #[error("{reason}")]
    Broken { reason: String },
}

impl From<SessionError> for LinkError {
    fn from(error: SessionError) -> Self {
        match error {
            SessionError::Version { mine, theirs } => Self::Version { mine, theirs },
            SessionError::NotPaired => Self::NotPaired,
            SessionError::Rejected => Self::Rejected,
            SessionError::Broken(reason) => Self::Broken {
                reason: reason.to_owned(),
            },
        }
    }
}

fn key(bytes: &[u8]) -> Result<[u8; KEY_LEN], LinkError> {
    bytes.try_into().map_err(|_| LinkError::Broken {
        reason: "키 길이가 틀림".to_owned(),
    })
}

/// 접속 하나를 맡는다. 새로 접속되면 새로 만든다.
#[derive(uniffi::Object)]
pub struct PhoneSession(Mutex<Session>);

/// 블루투스 알림 신호의 서비스 데이터(PSM + 알림 신호 지문).
#[uniffi::export]
pub fn ble_service_data(psm: u16, public_key: Vec<u8>) -> Result<Vec<u8>, LinkError> {
    Ok(crate::service_data(psm, &key(&public_key)?))
}

#[uniffi::export]
pub fn ble_service_uuid() -> String {
    crate::BLE_SERVICE.to_owned()
}

#[uniffi::export]
impl PhoneSession {
    #[uniffi::constructor]
    pub fn new(
        private_key: Vec<u8>,
        public_key: Vec<u8>,
        computers: Vec<Vec<u8>>,
        allow_pairing: bool,
    ) -> Result<Arc<Self>, LinkError> {
        let identity = Identity {
            private: key(&private_key)?,
            public: key(&public_key)?,
        };
        let trusted = computers
            .iter()
            .map(|computer| key(computer))
            .collect::<Result<_, _>>()?;
        let session = Session::new(Role::Phone, &identity, trusted, false);
        let session = if allow_pairing {
            session
        } else {
            session.refusing_pairing()
        };
        Ok(Arc::new(Self(Mutex::new(session))))
    }

    pub fn feed(&self, bytes: Vec<u8>) -> Result<(), LinkError> {
        Ok(self.lock().feed(&bytes)?)
    }

    pub fn take_output(&self) -> Vec<u8> {
        self.lock().take_output()
    }

    pub fn next_event(&self) -> Option<PhoneEvent> {
        let mut session = self.lock();
        loop {
            return match session.next_event()? {
                Event::PairingCode(code) => Some(PhoneEvent::PairingCode { code }),
                Event::Paired { peer } => Some(PhoneEvent::Paired {
                    computer_key: peer.to_vec(),
                }),
                Event::Ready { peer } => Some(PhoneEvent::Ready {
                    computer_key: peer.to_vec(),
                }),
                Event::Message(Body::Hello(hello)) => Some(PhoneEvent::ComputerName {
                    name: hello.computer_name,
                }),
                Event::Message(Body::Performance(value)) => Some(PhoneEvent::Performance {
                    value: value.into(),
                }),
                Event::Message(Body::Processes(Processes { items })) => {
                    Some(PhoneEvent::Processes {
                        items: items.into_iter().map(Into::into).collect(),
                    })
                }
                Event::Message(Body::CommandResult(CommandResult {
                    request_id,
                    ok,
                    reason,
                })) => Some(PhoneEvent::CommandResult {
                    request_id,
                    ok,
                    reason,
                }),
                Event::Message(_) => continue,
            };
        }
    }

    pub fn confirm(&self, same: bool) -> Result<(), LinkError> {
        Ok(self.lock().confirm(same)?)
    }

    /// 지금 보이는 것 전체를 알린다. 바뀔 때마다, 다시 붙을 때마다 보낸다.
    /// 화면이 꺼지거나 앱이 뒤로 가면 `main` 없이 빈 `tiles` 로 보낸다.
    pub fn subscribe(&self, main: Option<View>, tiles: Vec<View>) -> Result<(), LinkError> {
        let main = main.map_or(crate::View::Nothing, Into::into) as i32;
        let tiles = tiles
            .into_iter()
            .map(|view| crate::View::from(view) as i32)
            .collect();
        Ok(self
            .lock()
            .send(Body::Subscribe(Subscribe { main, tiles }))?)
    }

    /// 사람이 한 번 더 확인한 뒤에 부른다. 결과는 `CommandResult` 로 온다.
    pub fn kill_process(
        &self,
        request_id: u64,
        pid: u32,
        started_at: u64,
    ) -> Result<(), LinkError> {
        Ok(self.lock().send(Body::KillProcess(KillProcess {
            request_id,
            pid,
            started_at,
        }))?)
    }
}

impl PhoneSession {
    fn lock(&self) -> MutexGuard<'_, Session> {
        self.0.lock().unwrap_or_else(|lock| lock.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Hello;

    #[test]
    fn a_known_computer_sends_its_name() {
        let computer = Identity::generate();
        let phone = generate_key_pair();
        let phone_key: [u8; KEY_LEN] = phone.public_key.clone().try_into().unwrap();
        let session = PhoneSession::new(
            phone.private_key,
            phone.public_key,
            vec![computer.public.to_vec()],
            true,
        )
        .unwrap();
        let mut c = Session::new(Role::Computer, &computer, vec![phone_key], false);
        for _ in 0..3 {
            session.feed(c.take_output()).unwrap();
            c.feed(&session.take_output()).unwrap();
        }
        c.send(Body::Hello(Hello {
            computer_name: "맥".into(),
        }))
        .unwrap();
        session.feed(c.take_output()).unwrap();
        assert_eq!(
            session.next_event(),
            Some(PhoneEvent::Ready {
                computer_key: computer.public.to_vec()
            })
        );
        assert_eq!(
            session.next_event(),
            Some(PhoneEvent::ComputerName { name: "맥".into() })
        );
    }

    #[test]
    fn subscribes_kills_and_reads_values() {
        let computer = Identity::generate();
        let phone = generate_key_pair();
        let phone_key: [u8; KEY_LEN] = phone.public_key.clone().try_into().unwrap();
        let session = PhoneSession::new(
            phone.private_key,
            phone.public_key,
            vec![computer.public.to_vec()],
            true,
        )
        .unwrap();
        let mut c = Session::new(Role::Computer, &computer, vec![phone_key], false);
        for _ in 0..3 {
            session.feed(c.take_output()).unwrap();
            c.feed(&session.take_output()).unwrap();
        }
        session
            .subscribe(Some(View::Processes), vec![View::Performance])
            .unwrap();
        session.kill_process(9, 42, 1_000).unwrap();
        c.feed(&session.take_output()).unwrap();
        let mut got = Vec::new();
        while let Some(event) = c.next_event() {
            got.push(event);
        }
        assert!(got.contains(&Event::Message(Body::Subscribe(Subscribe {
            main: crate::View::Processes as i32,
            tiles: vec![crate::View::Performance as i32],
        }))));
        assert!(got.contains(&Event::Message(Body::KillProcess(KillProcess {
            request_id: 9,
            pid: 42,
            started_at: 1_000,
        }))));
        c.send(Body::Performance(Performance {
            cpu: 12.5,
            gpu: None,
            ..Default::default()
        }))
        .unwrap();
        session.feed(c.take_output()).unwrap();
        assert!(matches!(
            session.next_event(),
            Some(PhoneEvent::Ready { .. })
        ));
        assert!(matches!(
            session.next_event(),
            Some(PhoneEvent::Performance { value }) if value.cpu == 12.5 && value.gpu.is_none()
        ));
        assert_ne!(new_request_id(), new_request_id());
    }

    #[test]
    fn rejects_bad_keys() {
        assert!(PhoneSession::new(vec![0; 3], vec![0; 32], Vec::new(), true).is_err());
    }
}
