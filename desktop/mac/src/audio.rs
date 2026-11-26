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
/// 가상 주 볼륨(AudioHardwareService 속성)
const VIRTUAL_MAIN: u32 = four(b"vmvc");
/// 채널별 볼륨(0.0~1.0)
const VOLUME_SCALAR: u32 = four(b"volm");
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

fn read_f32(object: u32, address: &Address) -> Option<f32> {
    let mut value = 0f32;
    let mut size = size_of::<f32>() as u32;
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
    (status == 0 && value.is_finite()).then_some(value.clamp(0.0, 1.0))
}

fn settable(device: u32, address: &Address) -> bool {
    let mut settable = 0u8;
    // SAFETY: address 는 살아 있다.
    let status = unsafe { AudioObjectIsPropertySettable(device, address, &mut settable) };
    status == 0 && settable != 0
}

/// 볼륨 요소들: 가상 주 볼륨이 있으면 그것 하나, 없으면 채널 1, 2.
fn volume_elements(device: u32) -> Vec<Address> {
    let main = Address {
        selector: VIRTUAL_MAIN,
        scope: OUTPUT,
        element: MAIN,
    };
    // SAFETY: address 는 살아 있다.
    if unsafe { AudioObjectHasProperty(device, &main) } != 0 {
        return vec![main];
    }
    [1, 2]
        .iter()
        .map(|element| Address {
            selector: VOLUME_SCALAR,
            scope: OUTPUT,
            element: *element,
        })
        // SAFETY: address 는 살아 있다.
        .filter(|address| unsafe { AudioObjectHasProperty(device, address) } != 0)
        .collect()
}

pub fn to_percent(value: f32) -> u32 {
    (value.clamp(0.0, 1.0) * 100.0).round() as u32
}

/// 지금 볼륨(0~100, 채널이면 평균), 음소거, 바꿀 수 있는지. 장치가 없으면 없음.
pub fn volume() -> Option<sidedesk_domain::Level> {
    let device = output_device()?;
    let elements = volume_elements(device);
    let values: Vec<f32> = elements
        .iter()
        .filter_map(|address| read_f32(device, address))
        .collect();
    let level = if values.is_empty() {
        0
    } else {
        to_percent(values.iter().sum::<f32>() / values.len() as f32)
    };
    Some(sidedesk_domain::Level {
        level,
        muted: muted().unwrap_or(false),
        settable: !elements.is_empty() && elements.iter().all(|a| settable(device, a)),
    })
}

/// 볼륨을 0~100 으로 맞춘다.
pub fn set_volume(level: u32) -> Result<(), PortError> {
    let cannot = || PortError::new("볼륨을 바꿀 수 없는 장치입니다");
    let device =
        output_device().ok_or_else(|| PortError::new("소리 출력 장치를 찾지 못했습니다"))?;
    let elements = volume_elements(device);
    if elements.is_empty() || !elements.iter().all(|a| settable(device, a)) {
        return Err(cannot());
    }
    let value = level.min(100) as f32 / 100.0;
    for address in &elements {
        // SAFETY: value 는 f32 크기다.
        let status = unsafe {
            AudioObjectSetPropertyData(
                device,
                address,
                0,
                ptr::null(),
                size_of::<f32>() as u32,
                (&raw const value).cast(),
            )
        };
        if status != 0 {
            return Err(cannot());
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
        assert_eq!(VIRTUAL_MAIN, 0x766D_7663);
    }

    #[test]
    fn percent_rounding() {
        assert_eq!(to_percent(0.0), 0);
        assert_eq!(to_percent(0.494), 49);
        assert_eq!(to_percent(0.495), 50);
        assert_eq!(to_percent(1.2), 100);
    }

    #[test]
    #[ignore = "이 맥의 볼륨을 읽기만 한다. 손으로 돌린다"]
    fn reads_volume_only() {
        eprintln!("볼륨: {:?}", volume());
    }
}
