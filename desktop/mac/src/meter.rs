//! 맥 성능 수집. 공개 API(mach, sysctl, statfs, IOKit)만 쓴다.

use std::ffi::{CStr, c_char, c_void};
use std::mem::{MaybeUninit, size_of};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use core_foundation::base::{CFAllocatorRef, CFType, CFTypeRef, TCFType, kCFAllocatorDefault};
use core_foundation::dictionary::{CFDictionary, CFMutableDictionaryRef};
use core_foundation::number::CFNumber;
use core_foundation::string::{CFString, CFStringRef};
use sidedesk_core::Performance;
use sidedesk_domain::{Meter, PortError};

/// 앞 값이 이보다 오래되면(수집을 쉬었다 다시 시작) 짧게 두 번 재서 비율을 낸다.
pub(crate) const STALE: Duration = Duration::from_secs(5);
pub(crate) const WARM_UP: Duration = Duration::from_millis(250);

type IoObject = u32;

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOServiceMatching(name: *const c_char) -> CFMutableDictionaryRef;
    fn IOServiceGetMatchingServices(
        main_port: u32,
        matching: CFMutableDictionaryRef,
        existing: *mut IoObject,
    ) -> i32;
    fn IOIteratorNext(iterator: IoObject) -> IoObject;
    fn IORegistryEntryCreateCFProperty(
        entry: IoObject,
        key: CFStringRef,
        allocator: CFAllocatorRef,
        options: u32,
    ) -> CFTypeRef;
    fn IOObjectRelease(object: IoObject) -> i32;
}

#[derive(Clone, Copy)]
struct Sample {
    at: Instant,
    /// 바쁨, 전체
    cpu: (u64, u64),
    net: (u64, u64),
}

struct State {
    previous: Option<Sample>,
    /// 처음 찾은 GPU 항목. 못 읽게 되면 다시 찾는다.
    gpu: Option<IoObject>,
}

pub struct MacMeter {
    host: libc::mach_port_t,
    page: u64,
    memory_total: u64,
    state: Mutex<State>,
}

impl Default for MacMeter {
    fn default() -> Self {
        // SAFETY: 인자 없는 호출. 포트는 프로세스가 끝날 때까지 쓴다.
        #[allow(deprecated)]
        let host = unsafe { libc::mach_host_self() };
        // SAFETY: 인자 없는 호출
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) }.max(4096) as u64;
        Self {
            host,
            page,
            memory_total: sysctl_u64(c"hw.memsize").unwrap_or(0),
            state: Mutex::new(State {
                previous: None,
                gpu: None,
            }),
        }
    }
}

impl Meter for MacMeter {
    fn performance(&self) -> Result<Performance, PortError> {
        let mut state = self.state.lock().unwrap_or_else(|lock| lock.into_inner());
        let mut now = self.sample()?;
        let previous = match state.previous {
            Some(previous) if now.at.duration_since(previous.at) < STALE => previous,
            _ => {
                thread::sleep(WARM_UP);
                std::mem::replace(&mut now, self.sample()?)
            }
        };
        state.previous = Some(now);
        let seconds = now.at.duration_since(previous.at).as_secs_f64().max(0.001);
        let busy = now.cpu.0.saturating_sub(previous.cpu.0);
        let total = now.cpu.1.saturating_sub(previous.cpu.1);
        let rate = |now: u64, before: u64| (now.saturating_sub(before) as f64 / seconds) as u64;
        let memory_used = self.memory_used()?;
        Ok(Performance {
            cpu: percent(busy as f64, total as f64),
            gpu: gpu(&mut state.gpu),
            memory: percent(memory_used as f64, self.memory_total as f64),
            memory_used,
            memory_total: self.memory_total,
            disk: disk()?,
            net_down: rate(now.net.0, previous.net.0),
            net_up: rate(now.net.1, previous.net.1),
        })
    }
}

impl MacMeter {
    fn sample(&self) -> Result<Sample, PortError> {
        Ok(Sample {
            at: Instant::now(),
            cpu: self.cpu_ticks()?,
            net: network()?,
        })
    }

    fn cpu_ticks(&self) -> Result<(u64, u64), PortError> {
        let mut info = MaybeUninit::<libc::host_cpu_load_info>::zeroed();
        let mut count = libc::HOST_CPU_LOAD_INFO_COUNT;
        // SAFETY: info 는 count 만큼의 자리를 가진다.
        let result = unsafe {
            libc::host_statistics(
                self.host,
                libc::HOST_CPU_LOAD_INFO,
                info.as_mut_ptr().cast(),
                &mut count,
            )
        };
        if result != libc::KERN_SUCCESS {
            return Err(PortError::new("CPU 사용량을 읽지 못함"));
        }
        // SAFETY: 성공했으면 채워져 있다.
        let ticks = unsafe { info.assume_init() }.cpu_ticks.map(u64::from);
        let idle = ticks[libc::CPU_STATE_IDLE as usize];
        let total: u64 = ticks.iter().sum();
        Ok((total - idle, total))
    }

    /// 활동 상태 보기의 "사용된 메모리"와 같은 셈: 앱 메모리 + 고정 + 압축
    fn memory_used(&self) -> Result<u64, PortError> {
        let mut info = MaybeUninit::<libc::vm_statistics64>::zeroed();
        let mut count = libc::HOST_VM_INFO64_COUNT;
        // SAFETY: info 는 count 만큼의 자리를 가진다.
        let result = unsafe {
            libc::host_statistics64(
                self.host,
                libc::HOST_VM_INFO64,
                info.as_mut_ptr().cast(),
                &mut count,
            )
        };
        if result != libc::KERN_SUCCESS {
            return Err(PortError::new("메모리 사용량을 읽지 못함"));
        }
        // SAFETY: 성공했으면 채워져 있다.
        let vm = unsafe { info.assume_init() };
        let pages = u64::from(vm.internal_page_count).saturating_sub(u64::from(vm.purgeable_count))
            + u64::from(vm.wire_count)
            + u64::from(vm.compressor_page_count);
        Ok(pages * self.page)
    }
}

fn percent(part: f64, whole: f64) -> f32 {
    if whole <= 0.0 {
        return 0.0;
    }
    (part / whole * 100.0).clamp(0.0, 100.0) as f32
}

fn sysctl_u64(name: &CStr) -> Option<u64> {
    let mut value = 0u64;
    let mut size = size_of::<u64>();
    // SAFETY: value 는 size 바이트다.
    let result = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            (&raw mut value).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    (result == 0).then_some(value)
}

/// 시동 디스크(데이터 볼륨) 공간 사용률. df 의 Capacity 와 같은 셈.
fn disk() -> Result<f32, PortError> {
    for path in [c"/System/Volumes/Data", c"/"] {
        let mut info = MaybeUninit::<libc::statfs>::zeroed();
        // SAFETY: 경로는 널로 끝나고 info 는 statfs 크기다.
        if unsafe { libc::statfs(path.as_ptr(), info.as_mut_ptr()) } == 0 {
            // SAFETY: 성공했으면 채워져 있다.
            let info = unsafe { info.assume_init() };
            let used = info.f_blocks.saturating_sub(info.f_bfree) as f64;
            return Ok(percent(used, used + info.f_bavail as f64));
        }
    }
    Err(PortError::new("디스크 사용량을 읽지 못함"))
}

/// 루프백을 뺀 모든 연결의 받은 바이트, 보낸 바이트(64비트 계수)
fn network() -> Result<(u64, u64), PortError> {
    let broken = || PortError::new("네트워크 사용량을 읽지 못함");
    let mut mib = [libc::CTL_NET, libc::PF_ROUTE, 0, 0, libc::NET_RT_IFLIST2, 0];
    let mut size = 0usize;
    // SAFETY: 크기만 묻는다.
    let result = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            6,
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if result != 0 {
        return Err(broken());
    }
    // 그 사이 연결이 늘 수 있어 넉넉히 잡는다.
    size += 4096;
    let mut buffer = vec![0u8; size];
    // SAFETY: buffer 는 size 바이트다.
    let result = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            6,
            buffer.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if result != 0 {
        return Err(broken());
    }
    let mut total = (0u64, 0u64);
    let mut offset = 0;
    while offset + size_of::<libc::if_msghdr>() <= size {
        // SAFETY: 머리 크기만큼 남아 있음을 확인했다. 정렬이 맞지 않을 수 있어 그대로 읽는다.
        let header: libc::if_msghdr =
            unsafe { std::ptr::read_unaligned(buffer[offset..].as_ptr().cast()) };
        let length = usize::from(header.ifm_msglen);
        if length == 0 {
            break;
        }
        if i32::from(header.ifm_type) == libc::RTM_IFINFO2
            && offset + size_of::<libc::if_msghdr2>() <= size
        {
            // SAFETY: 종류가 RTM_IFINFO2 이고 크기가 남아 있다.
            let message: libc::if_msghdr2 =
                unsafe { std::ptr::read_unaligned(buffer[offset..].as_ptr().cast()) };
            if message.ifm_flags & libc::IFF_LOOPBACK == 0 {
                total.0 += message.ifm_data.ifi_ibytes;
                total.1 += message.ifm_data.ifi_obytes;
            }
        }
        offset += length;
    }
    Ok(total)
}

/// GPU 사용률(%). 읽지 못하면 없음.
fn gpu(cached: &mut Option<IoObject>) -> Option<f32> {
    if let Some(entry) = *cached {
        if let Some(value) = utilization(entry) {
            return Some(value);
        }
        // SAFETY: 우리가 잡아 둔 항목이다.
        unsafe { IOObjectRelease(entry) };
        *cached = None;
    }
    let mut iterator: IoObject = 0;
    // SAFETY: 이름은 널로 끝난다. matching 은 호출이 가져간다.
    let found = unsafe {
        let matching = IOServiceMatching(c"IOAccelerator".as_ptr());
        !matching.is_null() && IOServiceGetMatchingServices(0, matching, &mut iterator) == 0
    };
    if !found {
        return None;
    }
    let mut value = None;
    loop {
        // SAFETY: 위에서 얻은 반복자
        let entry = unsafe { IOIteratorNext(iterator) };
        if entry == 0 {
            break;
        }
        if value.is_none()
            && let Some(read) = utilization(entry)
        {
            value = Some(read);
            *cached = Some(entry);
            continue;
        }
        // SAFETY: 반복자가 준 항목은 우리가 놓는다.
        unsafe { IOObjectRelease(entry) };
    }
    // SAFETY: 위에서 얻은 반복자
    unsafe { IOObjectRelease(iterator) };
    value
}

fn utilization(entry: IoObject) -> Option<f32> {
    let key = CFString::from_static_string("PerformanceStatistics");
    // SAFETY: entry 는 살아 있는 항목이고, 돌려받은 값은 CFType 이 놓는다.
    let raw = unsafe {
        IORegistryEntryCreateCFProperty(entry, key.as_concrete_TypeRef(), kCFAllocatorDefault, 0)
    };
    if raw.is_null() {
        return None;
    }
    // SAFETY: 위에서 만든(create) 값
    let property = unsafe { CFType::wrap_under_create_rule(raw) };
    let statistics = property.downcast_into::<CFDictionary>()?;
    let name = CFString::from_static_string("Device Utilization %");
    let value = statistics.find(name.as_concrete_TypeRef().cast::<c_void>())?;
    // SAFETY: 사전 안의 값은 사전이 살아 있는 동안 유효하다.
    let value = unsafe { CFType::wrap_under_get_rule(*value) };
    let number = value.downcast::<CFNumber>()?.to_f64()?;
    Some(number.clamp(0.0, 100.0) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_in_range() {
        let meter = MacMeter::default();
        for _ in 0..2 {
            let p = meter.performance().unwrap();
            assert!((0.0..=100.0).contains(&p.cpu), "{p:?}");
            assert!((0.0..=100.0).contains(&p.memory), "{p:?}");
            assert!((0.0..=100.0).contains(&p.disk), "{p:?}");
            assert!(
                p.gpu.is_none_or(|gpu| (0.0..=100.0).contains(&gpu)),
                "{p:?}"
            );
            assert!(p.memory_total > 1 << 30);
            assert!(p.memory_used > 0 && p.memory_used <= p.memory_total);
            assert!(p.disk > 0.0);
            // 10Gbps 를 넘는 속도는 셈이 틀린 것
            assert!(
                p.net_down < 2_000_000_000 && p.net_up < 2_000_000_000,
                "{p:?}"
            );
        }
    }
}
