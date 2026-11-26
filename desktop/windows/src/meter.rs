//! 윈도우 성능 수집. Win32 공개 API(GetSystemTimes, GlobalMemoryStatusEx, GetDiskFreeSpaceExW,
//! GetIfTable2, PDH)만 쓴다.

use std::collections::HashMap;
use std::mem::size_of;
use std::ptr;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use sidedesk_core::Performance;
use sidedesk_domain::{Meter, PortError};
use windows_sys::Win32::Foundation::FILETIME;
use windows_sys::Win32::NetworkManagement::IpHelper::{
    FreeMibTable, GetIfTable2, IF_TYPE_SOFTWARE_LOOPBACK, MIB_IF_ROW2, MIB_IF_TABLE2,
};
use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use windows_sys::Win32::System::Performance::{
    PDH_CSTATUS_NEW_DATA, PDH_CSTATUS_VALID_DATA, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE,
    PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA, PdhAddEnglishCounterW, PdhCloseQuery,
    PdhCollectQueryData, PdhGetFormattedCounterArrayW, PdhOpenQueryW,
};
use windows_sys::Win32::System::SystemInformation::{
    GetSystemWindowsDirectoryW, GlobalMemoryStatusEx, MEMORYSTATUSEX,
};
use windows_sys::Win32::System::Threading::GetSystemTimes;

use crate::handle::{text, ticks, wide};

/// 앞 값이 이보다 오래되면(수집을 쉬었다 다시 시작) 짧게 두 번 재서 비율을 낸다. 맥과 같다.
pub(crate) const STALE: Duration = Duration::from_secs(5);
pub(crate) const WARM_UP: Duration = Duration::from_millis(250);
/// GPU 카운터를 열지 못했으면 이만큼 뒤에 다시 연다.
const GPU_RETRY: Duration = Duration::from_secs(60);

/// `MIB_IF_ROW2.InterfaceAndOperStatusFlags` 의 비트(ifdef.h 순서)
const HARDWARE: u8 = 1 << 0;
const FILTER: u8 = 1 << 1;

#[derive(Clone)]
struct Sample {
    at: Instant,
    /// 바쁨, 전체(100ns 단위)
    cpu: (u64, u64),
    /// 인터페이스 LUID → 받은 바이트, 보낸 바이트
    net: HashMap<u64, (u64, u64)>,
}

/// PDH 쿼리 하나와 GPU 엔진 카운터
struct Gpu {
    query: PDH_HQUERY,
    counter: PDH_HCOUNTER,
}

// SAFETY: PDH 함수는 스레드 안전하다(MS 문서). 이 값은 Mutex 안에서만 쓴다.
unsafe impl Send for Gpu {}

impl Drop for Gpu {
    fn drop(&mut self) {
        // SAFETY: open 에서 연 쿼리를 한 번만 닫는다. 카운터도 같이 닫힌다.
        unsafe { PdhCloseQuery(self.query) };
    }
}

impl Gpu {
    fn open() -> Option<Self> {
        let mut query: PDH_HQUERY = ptr::null_mut();
        // SAFETY: 자료원 null 은 실시간 값. query 를 채운다.
        if unsafe { PdhOpenQueryW(ptr::null(), 0, &mut query) } != 0 {
            return None;
        }
        let mut gpu = Self {
            query,
            counter: ptr::null_mut(),
        };
        let path = wide(r"\GPU Engine(*)\Utilization Percentage");
        let mut counter: PDH_HCOUNTER = ptr::null_mut();
        // SAFETY: 쿼리는 살아 있고 경로는 널로 끝난다.
        if unsafe { PdhAddEnglishCounterW(gpu.query, path.as_ptr(), 0, &mut counter) } != 0 {
            return None;
        }
        gpu.counter = counter;
        gpu.collect().then_some(gpu)
    }

    fn collect(&self) -> bool {
        // SAFETY: 살아 있는 쿼리
        unsafe { PdhCollectQueryData(self.query) == 0 }
    }

    /// 엔진 종류별 합 가운데 가장 큰 값(작업 관리자 셈). Err 는 카운터가 망가진 것.
    fn read(&self) -> Result<Option<f32>, ()> {
        for _ in 0..3 {
            let mut size = 0u32;
            let mut count = 0u32;
            // SAFETY: 버퍼 null 로 크기만 묻는다.
            let status = unsafe {
                PdhGetFormattedCounterArrayW(
                    self.counter,
                    PDH_FMT_DOUBLE,
                    &mut size,
                    &mut count,
                    ptr::null_mut(),
                )
            };
            if status == 0 && count == 0 {
                return Ok(None);
            }
            if status != PDH_MORE_DATA {
                return Err(());
            }
            // 항목 뒤에 이름 글자가 붙어 온다. 8바이트 정렬로 잡는다.
            let mut buffer = vec![0u64; (size as usize).div_ceil(8) + 1];
            let items = buffer.as_mut_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>();
            // SAFETY: buffer 는 size 바이트 이상이다.
            let status = unsafe {
                PdhGetFormattedCounterArrayW(
                    self.counter,
                    PDH_FMT_DOUBLE,
                    &mut size,
                    &mut count,
                    items,
                )
            };
            if status == PDH_MORE_DATA {
                // 그 사이 인스턴스가 늘었다. 다시 묻는다.
                continue;
            }
            if status != 0 {
                return Err(());
            }
            // SAFETY: 성공했으면 count 개가 채워져 있고 이름은 같은 버퍼 안을 가리킨다.
            let items = unsafe { std::slice::from_raw_parts(items, count as usize) };
            let values = items.iter().filter_map(|item| {
                let status = item.FmtValue.CStatus;
                if status != PDH_CSTATUS_VALID_DATA && status != PDH_CSTATUS_NEW_DATA {
                    return None;
                }
                // SAFETY: PDH 가 준 널 끝 이름
                let name = unsafe { wide_until_nul(item.szName) };
                // SAFETY: PDH_FMT_DOUBLE 로 받았다.
                let value = unsafe { item.FmtValue.Anonymous.doubleValue };
                Some((name, value))
            });
            return Ok(busiest_engine(values));
        }
        Err(())
    }
}

/// # Safety
/// `raw` 는 null 이거나 널로 끝나는 UTF-16 이어야 한다.
unsafe fn wide_until_nul(raw: *const u16) -> String {
    if raw.is_null() {
        return String::new();
    }
    let mut length = 0;
    // SAFETY: 널을 만날 때까지만 읽는다.
    while unsafe { *raw.add(length) } != 0 {
        length += 1;
    }
    // SAFETY: 위에서 센 길이
    String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(raw, length) })
}

/// 인스턴스 이름 `pid_.._engtype_3D` 의 엔진 종류별로 더하고 가장 큰 값
fn busiest_engine(values: impl Iterator<Item = (String, f64)>) -> Option<f32> {
    let mut sums: HashMap<String, f64> = HashMap::new();
    for (name, value) in values {
        let Some((_, kind)) = name.rsplit_once("engtype_") else {
            continue;
        };
        if value.is_finite() {
            *sums.entry(kind.to_owned()).or_default() += value;
        }
    }
    sums.into_values()
        .reduce(f64::max)
        .map(|value| value.clamp(0.0, 100.0) as f32)
}

struct State {
    previous: Option<Sample>,
    gpu: Option<Gpu>,
    /// GPU 를 열지 못했을 때 다시 열 시각
    gpu_retry: Option<Instant>,
}

pub struct WindowsMeter {
    memory_total: u64,
    state: Mutex<State>,
}

impl Default for WindowsMeter {
    fn default() -> Self {
        Self {
            memory_total: memory().map_or(0, |(total, _)| total),
            state: Mutex::new(State {
                previous: None,
                gpu: None,
                gpu_retry: None,
            }),
        }
    }
}

impl Meter for WindowsMeter {
    fn performance(&self) -> Result<Performance, PortError> {
        let mut state = self.state.lock().unwrap_or_else(|lock| lock.into_inner());
        if state.gpu.is_none() && state.gpu_retry.is_none_or(|at| Instant::now() >= at) {
            state.gpu = Gpu::open();
            state.gpu_retry = state.gpu.is_none().then(|| Instant::now() + GPU_RETRY);
        }
        let mut now = sample(state.gpu.as_ref())?;
        let previous = match state.previous.take() {
            Some(previous) if now.at.duration_since(previous.at) < STALE => previous,
            _ => {
                thread::sleep(WARM_UP);
                std::mem::replace(&mut now, sample(state.gpu.as_ref())?)
            }
        };
        let gpu = match state.gpu.as_ref().map(Gpu::read) {
            Some(Ok(value)) => value,
            Some(Err(())) => {
                // 읽기가 망가졌으면 닫고, 열기 실패와 같이 잠시 뒤에 다시 연다.
                state.gpu = None;
                state.gpu_retry = Some(Instant::now() + GPU_RETRY);
                None
            }
            None => None,
        };
        let seconds = now.at.duration_since(previous.at).as_secs_f64().max(0.001);
        let busy = now.cpu.0.saturating_sub(previous.cpu.0);
        let total = now.cpu.1.saturating_sub(previous.cpu.1);
        let (down, up) = net_delta(&previous.net, &now.net);
        let (_, available) = memory()?;
        let memory_used = self.memory_total.saturating_sub(available);
        let result = Performance {
            cpu: percent(busy as f64, total as f64),
            gpu,
            memory: percent(memory_used as f64, self.memory_total as f64),
            memory_used,
            memory_total: self.memory_total,
            disk: disk()?,
            net_down: (down as f64 / seconds) as u64,
            net_up: (up as f64 / seconds) as u64,
        };
        state.previous = Some(now);
        Ok(result)
    }
}

fn sample(gpu: Option<&Gpu>) -> Result<Sample, PortError> {
    if let Some(gpu) = gpu {
        // 실패는 read 에서 드러난다.
        gpu.collect();
    }
    Ok(Sample {
        at: Instant::now(),
        cpu: cpu_times()?,
        net: network()?,
    })
}

fn percent(part: f64, whole: f64) -> f32 {
    if whole <= 0.0 {
        return 0.0;
    }
    (part / whole * 100.0).clamp(0.0, 100.0) as f32
}

fn cpu_times() -> Result<(u64, u64), PortError> {
    let zero = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut idle, mut kernel, mut user) = (zero, zero, zero);
    // SAFETY: 세 값을 채우는 호출
    if unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) } == 0 {
        return Err(PortError::new("CPU 사용량을 읽지 못함"));
    }
    // 커널 시간에 쉬는 시간이 들어 있다.
    let total = ticks(kernel) + ticks(user);
    Ok((total.saturating_sub(ticks(idle)), total))
}

/// 전체, 쓸 수 있는 물리 메모리(바이트). 작업 관리자 "사용 중" = 전체 - 쓸 수 있는 것.
fn memory() -> Result<(u64, u64), PortError> {
    let mut status = MEMORYSTATUSEX {
        dwLength: size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: dwLength 를 채운 구조체
    if unsafe { GlobalMemoryStatusEx(&mut status) } == 0 {
        return Err(PortError::new("메모리 사용량을 읽지 못함"));
    }
    Ok((status.ullTotalPhys, status.ullAvailPhys))
}

/// 윈도우가 깔린 드라이브의 공간 사용률
fn disk() -> Result<f32, PortError> {
    let broken = || PortError::new("디스크 사용량을 읽지 못함");
    let mut folder = [0u16; 260];
    // SAFETY: 버퍼 길이를 그대로 넘긴다.
    let length = unsafe { GetSystemWindowsDirectoryW(folder.as_mut_ptr(), folder.len() as u32) };
    if length == 0 || length as usize >= folder.len() {
        return Err(broken());
    }
    let folder = text(&folder);
    // "C:\Windows" → "C:\"
    let root: String = folder.chars().take(3).collect();
    let root = wide(&root);
    let (mut available, mut total, mut free) = (0u64, 0u64, 0u64);
    // SAFETY: 널 끝 경로와 세 값
    if unsafe { GetDiskFreeSpaceExW(root.as_ptr(), &mut available, &mut total, &mut free) } == 0 {
        return Err(broken());
    }
    Ok(percent(total.saturating_sub(free) as f64, total as f64))
}

/// 실제 장치이고 필터가 아니며 루프백이 아닌 연결만 센다. 필터 줄은 같은 연결을 두 번 센다.
fn counted(flags: u8, kind: u32) -> bool {
    flags & HARDWARE != 0 && flags & FILTER == 0 && kind != IF_TYPE_SOFTWARE_LOOPBACK
}

fn network() -> Result<HashMap<u64, (u64, u64)>, PortError> {
    let mut table: *mut MIB_IF_TABLE2 = ptr::null_mut();
    // SAFETY: 표는 시스템이 만들고 우리가 FreeMibTable 로 놓는다.
    if unsafe { GetIfTable2(&mut table) } != 0 || table.is_null() {
        return Err(PortError::new("네트워크 사용량을 읽지 못함"));
    }
    struct Table(*mut MIB_IF_TABLE2);
    impl Drop for Table {
        fn drop(&mut self) {
            // SAFETY: GetIfTable2 가 준 표를 한 번만 놓는다.
            unsafe { FreeMibTable(self.0.cast()) };
        }
    }
    let table = Table(table);
    // SAFETY: 표 머리 뒤에 NumEntries 개의 줄이 이어진다.
    let rows: &[MIB_IF_ROW2] = unsafe {
        let head = &*table.0;
        std::slice::from_raw_parts(head.Table.as_ptr(), head.NumEntries as usize)
    };
    Ok(rows
        .iter()
        .filter(|row| counted(row.InterfaceAndOperStatusFlags._bitfield, row.Type))
        // SAFETY: LUID 는 64비트 값으로 읽어도 된다(공용체의 두 모양 모두 8바이트).
        .map(|row| {
            (
                unsafe { row.InterfaceLuid.Value },
                (row.InOctets, row.OutOctets),
            )
        })
        .collect())
}

/// 인터페이스별 차이의 합. 사라진 것은 빠지고, 새로 생긴 것은 앞 값이 없어 0, 줄어든 값은 0.
fn net_delta(before: &HashMap<u64, (u64, u64)>, now: &HashMap<u64, (u64, u64)>) -> (u64, u64) {
    now.iter().fold(
        (0, 0),
        |(down, up), (luid, (inbound, outbound))| match before.get(luid) {
            Some((was_in, was_out)) => (
                down + inbound.saturating_sub(*was_in),
                up + outbound.saturating_sub(*was_out),
            ),
            None => (down, up),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_in_range() {
        let meter = WindowsMeter::default();
        for _ in 0..2 {
            let start = Instant::now();
            let p = meter.performance().unwrap();
            eprintln!("성능 한 번 {:?}: {p:?}", start.elapsed());
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
            assert!(
                p.net_down < 2_000_000_000 && p.net_up < 2_000_000_000,
                "{p:?}"
            );
        }
    }

    #[test]
    fn counts_only_real_adapters() {
        assert!(counted(HARDWARE, 6));
        assert!(!counted(HARDWARE | FILTER, 6));
        assert!(!counted(0, 6));
        assert!(!counted(HARDWARE, IF_TYPE_SOFTWARE_LOOPBACK));
    }

    #[test]
    fn vanished_and_new_adapters_do_not_go_negative() {
        let before = HashMap::from([(1, (100, 100)), (2, (500, 500))]);
        let now = HashMap::from([(1, (150, 120)), (3, (900, 900))]);
        assert_eq!(net_delta(&before, &now), (50, 20));
        let reset = HashMap::from([(1, (10, 10))]);
        assert_eq!(net_delta(&before, &reset), (0, 0));
    }

    #[test]
    fn busiest_engine_kind_wins() {
        let values = [
            ("pid_1_luid_0x1_phys_0_eng_0_engtype_3D", 30.0),
            ("pid_2_luid_0x1_phys_0_eng_0_engtype_3D", 25.0),
            ("pid_1_luid_0x1_phys_0_eng_1_engtype_VideoDecode", 40.0),
            ("이상한 이름", 99.0),
        ]
        .map(|(name, value)| (name.to_owned(), value));
        assert_eq!(busiest_engine(values.into_iter()), Some(55.0));
        assert_eq!(busiest_engine(std::iter::empty()), None);
    }
}
