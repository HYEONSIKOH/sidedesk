//! 폰 앱(Kotlin, Swift)이 쓰는 연결.

use std::sync::{Arc, Mutex, MutexGuard};

use crate::{Body, Event, Identity, KEY_LEN, Role, Session, SessionError};

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
    PairingCode { code: String },
    Paired { computer_key: Vec<u8> },
    Ready { computer_key: Vec<u8> },
    ComputerName { name: String },
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
                Event::Message(_) => continue,
            };
        }
    }

    pub fn confirm(&self, same: bool) -> Result<(), LinkError> {
        Ok(self.lock().confirm(same)?)
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
    fn rejects_bad_keys() {
        assert!(PhoneSession::new(vec![0; 3], vec![0; 32], Vec::new(), true).is_err());
    }
}
