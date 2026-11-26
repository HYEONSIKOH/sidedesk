//! 윈도우 핸들과 글자 도우미

use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};

/// 닫아야 하는 커널 핸들. 놓을 때 닫는다.
pub(crate) struct Owned(pub(crate) HANDLE);

impl Owned {
    /// 실패 값(null, INVALID_HANDLE_VALUE)이면 없음
    pub(crate) fn new(handle: HANDLE) -> Option<Self> {
        (!handle.is_null() && handle != windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE)
            .then_some(Self(handle))
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: new 에서 유효한 핸들만 받았고 여기서 한 번만 닫는다.
        unsafe { CloseHandle(self.0) };
    }
}

/// 널로 끝나는 UTF-16
pub(crate) fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

/// 널 앞까지의 UTF-16 을 글자로
pub(crate) fn text(raw: &[u16]) -> String {
    let end = raw.iter().position(|c| *c == 0).unwrap_or(raw.len());
    String::from_utf16_lossy(&raw[..end])
}

pub(crate) fn ticks(time: FILETIME) -> u64 {
    (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_round_trip() {
        assert_eq!(text(&wide("가a")), "가a");
        assert_eq!(wide("a"), [97, 0]);
    }

    #[test]
    fn filetime_joins_halves() {
        let time = FILETIME {
            dwLowDateTime: 1,
            dwHighDateTime: 2,
        };
        assert_eq!(ticks(time), (2 << 32) | 1);
    }
}
