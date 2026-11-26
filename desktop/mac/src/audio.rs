//! 소리 끄기(CoreAudio). 기본 출력 장치를 매번 다시 찾는다(이어폰을 꽂으면 바뀜). 이 파일만 CoreAudio 를 부른다.

use std::ffi::c_void;
use std::mem::size_of;
use std::ptr;

use sidedesk_domain::PortError;

const SYSTEM: u32 = 1;
const fn four(code: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*code)
}
const DEFAULT_OUTPUT: u32 = four(b"dOut");
const GLOBAL: u32 = four(b"glob");
const OUTPUT: u32 = four(b"outp");
const MUTE: u32 = four(b"mute");
const MAIN: u32 = 0;

#[repr(C)]
struct Address {
    selector: u32,
    scope: u32,
    element: u32,
}

#[link(name = "CoreAudio", kind = "framework")]
unsafe extern "C" {
    fn AudioObjectGetPropertyData(
        object: u32,
        address: *const Address,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: *mut u32,
        data: *mut c_void,
    ) -> i32;
    fn AudioObjectSetPropertyData(
        object: u32,
        address: *const Address,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: u32,
        data: *const c_void,
    ) -> i32;
    fn AudioObjectHasProperty(object: u32, address: *const Address) -> u8;
    fn AudioObjectIsPropertySettable(
        object: u32,
        address: *const Address,
        settable: *mut u8,
    ) -> i32;
}

fn read_u32(object: u32, address: &Address) -> Option<u32> {
    let mut value = 0u32;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: value 는 size 바이트다.
    let status = unsafe {
        AudioObjectGetPropertyData(
            object,
            address,
            0,
            ptr::null(),
            &mut size,
            (&raw mut value).cast(),
        )
    };
    (status == 0).then_some(value)
}

fn output_device() -> Option<u32> {
    let address = Address {
        selector: DEFAULT_OUTPUT,
        scope: GLOBAL,
        element: MAIN,
    };
    read_u32(SYSTEM, &address).filter(|device| *device != 0)
}

/// 음소거를 쓸 수 있는 요소들. 주 요소에 없으면 왼쪽, 오른쪽 채널로.
fn mute_elements(device: u32) -> Vec<Address> {
    for elements in [&[MAIN][..], &[1, 2][..]] {
        let found: Vec<Address> = elements
            .iter()
            .map(|element| Address {
                selector: MUTE,
                scope: OUTPUT,
                element: *element,
            })
            // SAFETY: address 는 살아 있다.
            .filter(|address| unsafe { AudioObjectHasProperty(device, address) } != 0)
            .collect();
        if !found.is_empty() {
            return found;
        }
    }
    Vec::new()
}

/// 소리가 꺼져 있는지. 장치가 없거나 음소거를 못 하면 없음.
pub fn muted() -> Option<bool> {
    let device = output_device()?;
    let elements = mute_elements(device);
    let first = elements.first()?;
    read_u32(device, first).map(|value| value != 0)
}

/// 지금 상태를 뒤집는다. 쓸 수 있는지 모두 먼저 보고, 중간에 실패하면 앞엣것을 되돌린다.
pub fn toggle_mute() -> Result<(), PortError> {
    let failed = |text: &str| PortError::new(text);
    let device = output_device().ok_or_else(|| failed("소리 출력 장치를 찾지 못했습니다"))?;
    let elements = mute_elements(device);
    let now = elements
        .first()
        .and_then(|address| read_u32(device, address))
        .ok_or_else(|| failed("이 장치는 소리를 끌 수 없습니다"))?;
    for address in &elements {
        let mut settable = 0u8;
        // SAFETY: address 는 살아 있다.
        let status = unsafe { AudioObjectIsPropertySettable(device, address, &mut settable) };
        if status != 0 || settable == 0 {
            return Err(failed("이 장치는 소리를 끌 수 없습니다"));
        }
    }
    let write = |address: &Address, value: u32| {
        // SAFETY: value 는 u32 크기다.
        let status = unsafe {
            AudioObjectSetPropertyData(
                device,
                address,
                0,
                ptr::null(),
                size_of::<u32>() as u32,
                (&raw const value).cast(),
            )
        };
        status == 0
    };
    let next = u32::from(now == 0);
    for (index, address) in elements.iter().enumerate() {
        if !write(address, next) {
            for done in &elements[..index] {
                write(done, now);
            }
            return Err(failed("소리 장치를 바꾸지 못했습니다"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "이 맥의 소리 장치를 읽는다(바꾸지 않음). 손으로 돌린다"]
    fn reads_mute_only() {
        eprintln!("소리 꺼짐: {:?}", muted());
    }

    #[test]
    fn four_char_codes() {
        assert_eq!(MUTE, 0x6D75_7465);
    }
}
