use prost::Message;

pub const MAX_BODY: usize = 64 * 1024;
const HEADER: usize = 4;
const MAX_NAME: usize = 256;

#[derive(Clone, PartialEq, Message)]
pub struct Hello {
    #[prost(string, tag = "1")]
    pub computer_name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum WireError {
    #[error("빈 메시지")]
    Empty,
    #[error("메시지가 너무 큼")]
    TooLarge,
    #[error("읽을 수 없는 메시지")]
    Broken,
    #[error("컴퓨터 이름이 비어 있음")]
    NoName,
}

pub fn encode(hello: &Hello) -> Vec<u8> {
    let name = &hello.computer_name;
    let body = Hello {
        computer_name: name[..name.floor_char_boundary(MAX_NAME)].to_owned(),
    }
    .encode_to_vec();
    let mut frame = Vec::with_capacity(HEADER + body.len());
    frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
    frame.extend_from_slice(&body);
    frame
}

#[derive(Default)]
pub struct FrameReader {
    buffer: Vec<u8>,
    failed: Option<WireError>,
}

impl FrameReader {
    pub fn push(&mut self, bytes: &[u8]) {
        if self.failed.is_none() {
            self.buffer.extend_from_slice(bytes);
        }
    }

    pub fn pop(&mut self) -> Result<Option<Hello>, WireError> {
        if let Some(error) = self.failed {
            return Err(error);
        }
        self.take().inspect_err(|error| {
            self.failed = Some(*error);
            self.buffer = Vec::new();
        })
    }

    fn take(&mut self) -> Result<Option<Hello>, WireError> {
        let Some(header) = self.buffer.first_chunk::<HEADER>() else {
            return Ok(None);
        };
        let length = u32::from_be_bytes(*header) as usize;
        if length == 0 {
            return Err(WireError::Empty);
        }
        if length > MAX_BODY {
            return Err(WireError::TooLarge);
        }
        let end = HEADER + length;
        if self.buffer.len() < end {
            return Ok(None);
        }
        let hello = Hello::decode(&self.buffer[HEADER..end]).map_err(|_| WireError::Broken)?;
        self.buffer.drain(..end);
        if hello.computer_name.is_empty() {
            return Err(WireError::NoName);
        }
        Ok(Some(hello))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hello(name: &str) -> Hello {
        Hello {
            computer_name: name.to_owned(),
        }
    }

    fn frame(body: &[u8]) -> Vec<u8> {
        let mut frame = (body.len() as u32).to_be_bytes().to_vec();
        frame.extend_from_slice(body);
        frame
    }

    fn read(bytes: &[u8]) -> Result<Option<Hello>, WireError> {
        let mut reader = FrameReader::default();
        reader.push(bytes);
        reader.pop()
    }

    #[test]
    fn round_trip() {
        let sent = hello("현식의 맥북 💻");
        assert_eq!(read(&encode(&sent)), Ok(Some(sent)));
    }

    #[test]
    fn one_byte_at_a_time() {
        let bytes = encode(&hello("mac"));
        let mut reader = FrameReader::default();
        for byte in &bytes[..bytes.len() - 1] {
            reader.push(&[*byte]);
            assert_eq!(reader.pop(), Ok(None));
        }
        reader.push(&bytes[bytes.len() - 1..]);
        assert_eq!(reader.pop(), Ok(Some(hello("mac"))));
    }

    #[test]
    fn two_at_once() {
        let mut reader = FrameReader::default();
        reader.push(&[encode(&hello("a")), encode(&hello("b"))].concat());
        assert_eq!(reader.pop(), Ok(Some(hello("a"))));
        assert_eq!(reader.pop(), Ok(Some(hello("b"))));
        assert_eq!(reader.pop(), Ok(None));
    }

    #[test]
    fn rejects_bad_lengths_from_the_header_alone() {
        assert_eq!(read(&[0; 4]), Err(WireError::Empty));
        assert_eq!(read(&[0xff; 4]), Err(WireError::TooLarge));
        let over = (MAX_BODY as u32 + 1).to_be_bytes();
        assert_eq!(read(&over), Err(WireError::TooLarge));
        let limit = (MAX_BODY as u32).to_be_bytes();
        assert_eq!(read(&limit), Ok(None));
    }

    #[test]
    fn rejects_broken_and_nameless_bodies() {
        assert_eq!(read(&frame(&[0x0a, 0x7f])), Err(WireError::Broken));
        assert_eq!(read(&frame(&[0x10, 0x01])), Err(WireError::NoName));
    }

    #[test]
    fn ignores_unknown_fields() {
        let body = [hello("mac").encode_to_vec(), vec![0x10, 0x01]].concat();
        assert_eq!(read(&frame(&body)), Ok(Some(hello("mac"))));
    }

    #[test]
    fn stays_failed() {
        let mut reader = FrameReader::default();
        reader.push(&[0; 4]);
        assert_eq!(reader.pop(), Err(WireError::Empty));
        reader.push(&encode(&hello("mac")));
        assert_eq!(reader.pop(), Err(WireError::Empty));
        assert!(reader.buffer.is_empty());
    }

    #[test]
    fn cuts_long_names_between_characters() {
        let cut = read(&encode(&hello(&"가".repeat(100)))).unwrap().unwrap();
        assert_eq!(cut.computer_name, "가".repeat(85));
    }
}
