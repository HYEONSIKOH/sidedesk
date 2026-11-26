//! 키 조합 보내기(CoreGraphics). 이 파일만 CGEvent 를 부른다.

use std::ffi::c_void;

use core_foundation::base::{CFRelease, CFType, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::string::CFString;
use sidedesk_domain::{Combo, PortError};

const PRIVATE_STATE: i32 = -1;
const HID_TAP: u32 = 0;
const SHIFT: u64 = 0x0002_0000;
const CONTROL: u64 = 0x0004_0000;
const OPTION: u64 = 0x0008_0000;
const COMMAND: u64 = 0x0010_0000;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventSourceCreate(state: i32) -> *mut c_void;
    fn CGEventCreateKeyboardEvent(source: *mut c_void, key: u16, down: bool) -> *mut c_void;
    fn CGEventSetFlags(event: *mut c_void, flags: u64);
    fn CGEventPost(tap: u32, event: *mut c_void);
    fn CGPreflightPostEventAccess() -> bool;
    fn CGSessionCopyCurrentDictionary() -> CFDictionaryRef;
}

/// 맥 가상 키 번호(브라우저 KeyboardEvent.code 이름에서)
pub fn key_code(code: &str) -> Option<u16> {
    let letters = [
        ("KeyA", 0x00),
        ("KeyS", 0x01),
        ("KeyD", 0x02),
        ("KeyF", 0x03),
        ("KeyH", 0x04),
        ("KeyG", 0x05),
        ("KeyZ", 0x06),
        ("KeyX", 0x07),
        ("KeyC", 0x08),
        ("KeyV", 0x09),
        ("IntlBackslash", 0x0A),
        ("KeyB", 0x0B),
        ("KeyQ", 0x0C),
        ("KeyW", 0x0D),
        ("KeyE", 0x0E),
        ("KeyR", 0x0F),
        ("KeyY", 0x10),
        ("KeyT", 0x11),
        ("Digit1", 0x12),
        ("Digit2", 0x13),
        ("Digit3", 0x14),
        ("Digit4", 0x15),
        ("Digit6", 0x16),
        ("Digit5", 0x17),
        ("Equal", 0x18),
        ("Digit9", 0x19),
        ("Digit7", 0x1A),
        ("Minus", 0x1B),
        ("Digit8", 0x1C),
        ("Digit0", 0x1D),
        ("BracketRight", 0x1E),
        ("KeyO", 0x1F),
        ("KeyU", 0x20),
        ("BracketLeft", 0x21),
        ("KeyI", 0x22),
        ("KeyP", 0x23),
        ("Enter", 0x24),
        ("KeyL", 0x25),
        ("KeyJ", 0x26),
        ("Quote", 0x27),
        ("KeyK", 0x28),
        ("Semicolon", 0x29),
        ("Backslash", 0x2A),
        ("Comma", 0x2B),
        ("Slash", 0x2C),
        ("KeyN", 0x2D),
        ("KeyM", 0x2E),
        ("Period", 0x2F),
        ("Tab", 0x30),
        ("Space", 0x31),
        ("Backquote", 0x32),
        ("Backspace", 0x33),
        ("Escape", 0x35),
        ("F5", 0x60),
        ("F6", 0x61),
        ("F7", 0x62),
        ("F3", 0x63),
        ("F8", 0x64),
        ("F9", 0x65),
        ("F11", 0x67),
        ("F10", 0x6D),
        ("F12", 0x6F),
        ("Home", 0x73),
        ("PageUp", 0x74),
        ("Delete", 0x75),
        ("F4", 0x76),
        ("End", 0x77),
        ("F2", 0x78),
        ("PageDown", 0x79),
        ("F1", 0x7A),
        ("ArrowLeft", 0x7B),
        ("ArrowRight", 0x7C),
        ("ArrowDown", 0x7D),
        ("ArrowUp", 0x7E),
    ];
    letters
        .iter()
        .find(|(name, _)| *name == code)
        .map(|(_, key)| *key)
}

/// 키를 보낼 권한(손쉬운 사용)이 있는지
pub fn allowed() -> bool {
    // SAFETY: 인자 없는 물음
    unsafe { CGPreflightPostEventAccess() }
}

/// 화면이 잠겨 있는지. 문서에 없는 키라 없으면 풀림으로 본다.
pub fn screen_locked() -> bool {
    // SAFETY: 돌려받은 사전은 create 규칙이라 CFDictionary 가 놓는다.
    let raw = unsafe { CGSessionCopyCurrentDictionary() };
    if raw.is_null() {
        return false;
    }
    // SAFETY: 위에서 만든(copy) 사전
    let dictionary: CFDictionary<CFString, CFType> =
        unsafe { CFDictionary::wrap_under_create_rule(raw) };
    dictionary
        .find(CFString::from_static_string("CGSSessionScreenIsLocked"))
        .and_then(|value| value.downcast::<CFBoolean>())
        .is_some_and(bool::from)
}

/// 앞에 있는 창에 키 조합을 보낸다. 누름과 뗌을 둘 다 만든 뒤에만 보낸다(누름만 남지 않게).
pub fn send(combo: &Combo) -> Result<(), PortError> {
    if !allowed() {
        return Err(PortError::new(
            "키를 보낼 권한이 없습니다. 설정 창의 안내를 보세요",
        ));
    }
    if screen_locked() {
        return Err(PortError::new("화면이 잠겨 있어 키를 보내지 않았습니다"));
    }
    let key = key_code(&combo.code).ok_or_else(|| PortError::new("보낼 수 없는 키입니다"))?;
    let flags = [
        (combo.command, COMMAND),
        (combo.control, CONTROL),
        (combo.option, OPTION),
        (combo.shift, SHIFT),
    ]
    .iter()
    .filter(|(on, _)| *on)
    .fold(0, |all, (_, flag)| all | flag);
    // SAFETY: 사람이 누르고 있는 수식 키가 섞이지 않게 따로 출처를 만든다. 실패하면 null.
    let source = unsafe { CGEventSourceCreate(PRIVATE_STATE) };
    if source.is_null() {
        return Err(PortError::new("실행하지 못했습니다"));
    }
    // SAFETY: source 는 살아 있다. 만든 이벤트는 아래에서 놓는다.
    let (down, up) = unsafe {
        (
            CGEventCreateKeyboardEvent(source, key, true),
            CGEventCreateKeyboardEvent(source, key, false),
        )
    };
    let sent = !down.is_null() && !up.is_null();
    if sent {
        // SAFETY: 두 이벤트 모두 null 이 아니다.
        unsafe {
            CGEventSetFlags(down, flags);
            CGEventSetFlags(up, flags);
            CGEventPost(HID_TAP, down);
            CGEventPost(HID_TAP, up);
        }
    }
    // SAFETY: 만든 것만 놓는다.
    unsafe {
        for event in [down, up] {
            if !event.is_null() {
                CFRelease(event);
            }
        }
        CFRelease(source);
    }
    if sent {
        Ok(())
    } else {
        Err(PortError::new("실행하지 못했습니다"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_allowed_code_has_a_key() {
        for code in sidedesk_domain::KEY_CODES {
            assert!(key_code(code).is_some(), "{code}");
        }
        assert_eq!(key_code("KeyQ"), Some(0x0C));
        assert_eq!(key_code("Nope"), None);
    }

    #[test]
    fn asking_does_not_send_anything() {
        // 권한과 잠금 물음은 키를 보내지 않는다.
        let _ = allowed();
        let _ = screen_locked();
    }
}
