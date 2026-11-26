pub const MAX_BODY: usize = u16::MAX as usize;
const HEADER: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum WireError {
    #[error("빈 메시지")]
    Empty,
    #[error("메시지가 너무 큼")]
    TooLarge,
}

pub fn frame(body: &[u8]) -> Vec<u8> {
    debug_assert!(!body.is_empty() && body.len() <= MAX_BODY);
    let mut frame = Vec::with_capacity(HEADER + body.len());
    frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
    frame.extend_from_slice(body);
    frame
}

/// 받은 바이트를 쌓아 두었다가 다 모인 본문을 하나씩 돌려준다.
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

    pub fn pop(&mut self) -> Result<Option<Vec<u8>>, WireError> {
        if let Some(error) = self.failed {
            return Err(error);
        }
        self.take().inspect_err(|error| {
            self.failed = Some(*error);
            self.buffer = Vec::new();
        })
    }

    fn take(&mut self) -> Result<Option<Vec<u8>>, WireError> {
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
        if self.buffer.len() < HEADER + length {
            return Ok(None);
        }
        let body = self.buffer[HEADER..HEADER + length].to_vec();
        self.buffer.drain(..HEADER + length);
        Ok(Some(body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(bytes: &[u8]) -> Result<Option<Vec<u8>>, WireError> {
        let mut reader = FrameReader::default();
        reader.push(bytes);
        reader.pop()
    }

    #[test]
    fn round_trip() {
        assert_eq!(read(&frame(b"hi")), Ok(Some(b"hi".to_vec())));
    }

    #[test]
    fn one_byte_at_a_time() {
        let bytes = frame(b"hello");
        let mut reader = FrameReader::default();
        for byte in &bytes[..bytes.len() - 1] {
            reader.push(&[*byte]);
            assert_eq!(reader.pop(), Ok(None));
        }
        reader.push(&bytes[bytes.len() - 1..]);
        assert_eq!(reader.pop(), Ok(Some(b"hello".to_vec())));
    }

    #[test]
    fn two_at_once() {
        let mut reader = FrameReader::default();
        reader.push(&[frame(b"a"), frame(b"b")].concat());
        assert_eq!(reader.pop(), Ok(Some(b"a".to_vec())));
        assert_eq!(reader.pop(), Ok(Some(b"b".to_vec())));
        assert_eq!(reader.pop(), Ok(None));
    }

    #[test]
    fn rejects_bad_lengths_from_the_header_alone() {
        assert_eq!(read(&[0; 4]), Err(WireError::Empty));
        assert_eq!(read(&[0xff; 4]), Err(WireError::TooLarge));
        assert_eq!(
            read(&(MAX_BODY as u32 + 1).to_be_bytes()),
            Err(WireError::TooLarge)
        );
        assert_eq!(read(&(MAX_BODY as u32).to_be_bytes()), Ok(None));
    }

    #[test]
    fn stays_failed() {
        let mut reader = FrameReader::default();
        reader.push(&[0; 4]);
        assert_eq!(reader.pop(), Err(WireError::Empty));
        reader.push(&frame(b"a"));
        assert_eq!(reader.pop(), Err(WireError::Empty));
        assert!(reader.buffer.is_empty());
    }
}
