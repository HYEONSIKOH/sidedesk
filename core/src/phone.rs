use std::sync::Mutex;

use crate::{FrameReader, WireError};

#[derive(Debug, PartialEq, thiserror::Error, uniffi::Error)]
pub enum ReceiveError {
    #[error("{reason}")]
    Broken { reason: String },
}

impl From<WireError> for ReceiveError {
    fn from(error: WireError) -> Self {
        Self::Broken {
            reason: error.to_string(),
        }
    }
}

/// 접속 하나를 맡는다. 새로 접속되면 새로 만든다.
#[derive(Default, uniffi::Object)]
pub struct PhoneReceiver {
    reader: Mutex<FrameReader>,
}

#[uniffi::export]
impl PhoneReceiver {
    #[uniffi::constructor]
    pub fn new() -> Self {
        Self::default()
    }

    /// 다 모인 컴퓨터 이름을 돌려준다. 깨진 메시지 앞에 온 이름은 돌려주고 오류는 다음 호출에서 낸다.
    pub fn receive(&self, bytes: Vec<u8>) -> Result<Vec<String>, ReceiveError> {
        let mut reader = self.reader.lock().unwrap_or_else(|lock| lock.into_inner());
        reader.push(&bytes);
        let mut names = Vec::new();
        loop {
            match reader.pop() {
                Ok(Some(hello)) => names.push(hello.computer_name),
                Ok(None) => return Ok(names),
                Err(error) if names.is_empty() => return Err(error.into()),
                Err(_) => return Ok(names),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Hello, encode};

    fn hello(name: &str) -> Vec<u8> {
        encode(&Hello {
            computer_name: name.to_owned(),
        })
    }

    #[test]
    fn returns_good_names_before_the_error() {
        let receiver = PhoneReceiver::new();
        let bytes = [hello("a"), vec![0; 4]].concat();
        assert_eq!(receiver.receive(bytes), Ok(vec!["a".to_owned()]));
        assert!(receiver.receive(Vec::new()).is_err());
    }

    #[test]
    fn a_broken_connection_does_not_affect_the_next() {
        assert!(PhoneReceiver::new().receive(vec![0; 4]).is_err());
        assert_eq!(
            PhoneReceiver::new().receive(hello("b")),
            Ok(vec!["b".to_owned()])
        );
    }
}
