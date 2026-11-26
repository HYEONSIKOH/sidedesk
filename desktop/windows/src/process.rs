//! 윈도우 프로세스 목록과 종료. 이 프로그램을 돌리는 사용자의 프로세스만 다룬다(맥과 같은 뜻).

use std::collections::HashMap;
use std::mem::size_of;
use std::sync::Mutex;
use std::thread;
use std::time::Instant;

use sidedesk_core::Process;
use sidedesk_domain::{PortError, ProcessTable};
use windows_sys::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, FILETIME, GetLastError, HANDLE, WAIT_OBJECT_0,
};
use windows_sys::Win32::Security::{
    EqualSid, GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::ProcessStatus::{
    GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX2,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetProcessTimes, OpenProcess, OpenProcessToken,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE, TerminateProcess, WaitForSingleObject,
};

use crate::handle::{Owned, text, ticks};
use crate::meter::{STALE, WARM_UP};

/// 1601-01-01 부터 1970-01-01 까지(100ns 단위)
const UNIX_EPOCH: u64 = 116_444_736_000_000_000;

struct Read {
    pid: u32,
    name: String,
    started_at: u64,
    /// 100ns 단위
    cpu_time: u64,
    memory: u64,
}

struct Previous {
    at: Instant,
    /// (pid, 시작 시각) → CPU 시간(100ns)
    cpu: HashMap<(u32, u64), u64>,
}

/// 토큰의 사용자 정보. SID 가 같은 버퍼 안을 가리키므로 버퍼째 들고 있는다.
struct User(Vec<u64>);

impl User {
    fn of(process: HANDLE) -> Option<Self> {
        let mut token = std::ptr::null_mut();
        // SAFETY: 살아 있는 프로세스 핸들, token 을 채운다.
        if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
            return None;
        }
        let token = Owned::new(token)?;
        let mut size = 0u32;
        // SAFETY: 크기만 묻는다.
        unsafe { GetTokenInformation(token.0, TokenUser, std::ptr::null_mut(), 0, &mut size) };
        if (size as usize) < size_of::<TOKEN_USER>() {
            return None;
        }
        let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
        // SAFETY: buffer 는 size 바이트 이상이고 8바이트 정렬이다.
        let read = unsafe {
            GetTokenInformation(
                token.0,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                size,
                &mut size,
            )
        };
        (read != 0).then_some(Self(buffer))
    }

    fn same(&self, other: &Self) -> bool {
        // SAFETY: 두 버퍼는 GetTokenInformation(TokenUser)이 채운 TOKEN_USER 로 시작하고 SID 는 그 안을 가리킨다.
        unsafe {
            let mine = &*self.0.as_ptr().cast::<TOKEN_USER>();
            let theirs = &*other.0.as_ptr().cast::<TOKEN_USER>();
            EqualSid(mine.User.Sid, theirs.User.Sid) != 0
        }
    }
}

// SAFETY: 버퍼는 우리 것이고 그 안의 SID 포인터도 같은 버퍼를 가리킨다. 읽기만 한다.
unsafe impl Send for User {}
// SAFETY: 위와 같다.
unsafe impl Sync for User {}

pub struct WindowsProcesses {
    /// 이 프로그램을 돌리는 사용자. 못 읽으면 아무 프로세스도 보이지 않는다.
    me: Option<User>,
    previous: Mutex<Option<Previous>>,
}

impl Default for WindowsProcesses {
    fn default() -> Self {
        Self {
            // SAFETY: 자기 프로세스의 가짜 핸들은 닫지 않아도 된다.
            me: User::of(unsafe { GetCurrentProcess() }),
            previous: Mutex::new(None),
        }
    }
}

fn gone() -> PortError {
    PortError::new("프로세스가 이미 끝났거나 다른 프로세스로 바뀌었습니다")
}

impl ProcessTable for WindowsProcesses {
    fn processes(&self) -> Result<Vec<Process>, PortError> {
        let mut guard = self
            .previous
            .lock()
            .unwrap_or_else(|lock| lock.into_inner());
        let mut at = Instant::now();
        let mut now = self.read_all()?;
        let before = match guard.take() {
            Some(previous) if at.duration_since(previous.at) < STALE => previous,
            _ => {
                let first = Previous {
                    at,
                    cpu: cpu_map(&now),
                };
                thread::sleep(WARM_UP);
                at = Instant::now();
                now = self.read_all()?;
                first
            }
        };
        // 100ns 단위 벽시계
        let wall = (at.duration_since(before.at).as_nanos() / 100).max(1) as f64;
        let processes = now
            .iter()
            .map(|read| {
                let used = before
                    .cpu
                    .get(&(read.pid, read.started_at))
                    .map_or(0, |was| read.cpu_time.saturating_sub(*was));
                Process {
                    pid: read.pid,
                    name: read.name.clone(),
                    cpu: (used as f64 / wall * 100.0) as f32,
                    memory: read.memory,
                    started_at: read.started_at,
                }
            })
            .collect();
        *guard = Some(Previous {
            at,
            cpu: cpu_map(&now),
        });
        Ok(processes)
    }

    fn kill(&self, pid: u32, started_at: u64) -> Result<(), PortError> {
        // SAFETY: 번호로 여는 호출. 실패하면 null.
        let raw = unsafe {
            OpenProcess(
                PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
                0,
                pid,
            )
        };
        let Some(process) = Owned::new(raw) else {
            // SAFETY: 바로 앞 호출의 오류
            return Err(match unsafe { GetLastError() } {
                ERROR_ACCESS_DENIED => PortError::new("이 프로세스를 끝낼 권한이 없습니다"),
                ERROR_INVALID_PARAMETER => gone(),
                _ => PortError::new("알 수 없는 오류가 났습니다"),
            });
        };
        // 핸들을 쥔 동안에는 번호가 다른 프로세스에 다시 쓰이지 않는다.
        let (start, _) = times(process.0).ok_or_else(gone)?;
        if start != started_at {
            return Err(gone());
        }
        // 목록에 보이는 것(같은 사용자)만 끝낸다. 폰이 아무 번호나 보내도 남의 프로세스는 건드리지 않는다.
        if !self.mine(process.0) {
            return Err(PortError::new("이 프로세스를 끝낼 권한이 없습니다"));
        }
        // SAFETY: 끝내기 권한으로 연 핸들
        if unsafe { TerminateProcess(process.0, 1) } != 0 {
            return Ok(());
        }
        // SAFETY: 바로 앞 호출의 오류
        let error = unsafe { GetLastError() };
        // SAFETY: 살아 있는 핸들을 기다리지 않고 본다.
        if unsafe { WaitForSingleObject(process.0, 0) } == WAIT_OBJECT_0 {
            return Err(gone());
        }
        Err(match error {
            ERROR_ACCESS_DENIED => PortError::new("이 프로세스를 끝낼 권한이 없습니다"),
            _ => PortError::new("알 수 없는 오류가 났습니다"),
        })
    }
}

impl WindowsProcesses {
    fn mine(&self, process: HANDLE) -> bool {
        match (&self.me, User::of(process)) {
            (Some(me), Some(them)) => me.same(&them),
            _ => false,
        }
    }

    fn read_all(&self) -> Result<Vec<Read>, PortError> {
        Ok(entries()?
            .into_iter()
            .filter_map(|(pid, name)| self.read(pid, name))
            .collect())
    }

    fn read(&self, pid: u32, name: String) -> Option<Read> {
        // SAFETY: 번호로 여는 호출. 못 여는 프로세스(시스템, 보호, 남의 것)는 null.
        let process =
            Owned::new(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) })?;
        if !self.mine(process.0) {
            return None;
        }
        let (started_at, cpu_time) = times(process.0)?;
        Some(Read {
            pid,
            name,
            started_at,
            cpu_time,
            memory: memory(process.0)?,
        })
    }
}

fn cpu_map(reads: &[Read]) -> HashMap<(u32, u64), u64> {
    reads
        .iter()
        .map(|read| ((read.pid, read.started_at), read.cpu_time))
        .collect()
}

/// 번호와 이름(끝의 .exe 를 뗀 것)
fn entries() -> Result<Vec<(u32, String)>, PortError> {
    let broken = || PortError::new("프로세스 목록을 읽지 못함");
    // SAFETY: 모든 프로세스의 스냅숏. 실패하면 INVALID_HANDLE_VALUE.
    let snapshot = Owned::new(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) })
        .ok_or_else(broken)?;
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut list = Vec::new();
    // SAFETY: dwSize 를 채운 구조체
    let mut more = unsafe { Process32FirstW(snapshot.0, &mut entry) } != 0;
    if !more {
        return Err(broken());
    }
    while more {
        if entry.th32ProcessID != 0 {
            list.push((entry.th32ProcessID, short_name(&text(&entry.szExeFile))));
        }
        // SAFETY: 위와 같다.
        more = unsafe { Process32NextW(snapshot.0, &mut entry) } != 0;
    }
    Ok(list)
}

fn short_name(file: &str) -> String {
    match file.len().checked_sub(4) {
        Some(cut) if file.is_char_boundary(cut) && file[cut..].eq_ignore_ascii_case(".exe") => {
            file[..cut].to_owned()
        }
        _ => file.to_owned(),
    }
}

/// 시작 시각(유닉스 밀리초), 커널 + 사용자 CPU 시간(100ns)
fn times(process: HANDLE) -> Option<(u64, u64)> {
    let zero = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
    // SAFETY: 조회 권한으로 연 핸들과 네 값
    let read =
        unsafe { GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user) };
    (read != 0).then(|| {
        (
            ticks(created).saturating_sub(UNIX_EPOCH) / 10_000,
            ticks(kernel) + ticks(user),
        )
    })
}

/// 작업 관리자 "메모리" 열(전용 작업 집합). 그 값을 모르는 옛 윈도우는 작업 집합.
fn memory(process: HANDLE) -> Option<u64> {
    let mut counters = PROCESS_MEMORY_COUNTERS_EX2 {
        cb: size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32,
        ..Default::default()
    };
    // SAFETY: cb 만큼의 구조체. EX2 는 앞부분이 PROCESS_MEMORY_COUNTERS 와 같다.
    let read = unsafe {
        GetProcessMemoryInfo(
            process,
            (&raw mut counters).cast::<PROCESS_MEMORY_COUNTERS>(),
            counters.cb,
        )
    };
    if read != 0 {
        return Some(counters.PrivateWorkingSetSize as u64);
    }
    let mut basic = PROCESS_MEMORY_COUNTERS {
        cb: size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        ..Default::default()
    };
    // SAFETY: cb 만큼의 구조체
    let read = unsafe { GetProcessMemoryInfo(process, &mut basic, basic.cb) };
    (read != 0).then_some(basic.WorkingSetSize as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn lists_this_process_with_sane_values() {
        let table = WindowsProcesses::default();
        let start = Instant::now();
        let list = table.processes().unwrap();
        eprintln!("프로세스 {}개 {:?}", list.len(), start.elapsed());
        let start = Instant::now();
        table.processes().unwrap();
        eprintln!("두 번째 {:?}", start.elapsed());
        let me = std::process::id();
        let mine = list.iter().find(|p| p.pid == me).expect("내 프로세스");
        assert!(!mine.name.is_empty());
        assert!(!mine.name.to_ascii_lowercase().ends_with(".exe"));
        assert!(mine.memory > 0);
        assert!(mine.started_at > 1_600_000_000_000);
        let cores = thread::available_parallelism().map_or(64, |n| n.get()) as f32;
        assert!(
            list.iter()
                .all(|p| p.cpu >= 0.0 && p.cpu <= cores * 100.0 + 1.0)
        );
        // 시스템 프로세스(번호 4)는 이 사용자 것이 아니다.
        assert!(list.iter().all(|p| p.pid != 4));
    }

    #[test]
    fn kills_only_when_the_start_time_matches() {
        let mut child = Command::new("ping")
            .args(["-n", "30", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();
        let table = WindowsProcesses::default();
        let started = table
            .processes()
            .unwrap()
            .into_iter()
            .find(|p| p.pid == pid)
            .map(|p| p.started_at);
        let Some(started) = started else {
            let _ = child.kill();
            let _ = child.wait();
            panic!("자식 프로세스가 목록에 없음");
        };
        assert!(table.kill(pid, started + 1).is_err());
        assert!(child.try_wait().unwrap().is_none());
        table.kill(pid, started).unwrap();
        let status = child.wait().unwrap();
        assert!(!status.success());
        assert!(table.kill(pid, started).is_err());
    }

    #[test]
    fn system_processes_are_refused() {
        assert!(WindowsProcesses::default().kill(4, 0).is_err());
    }

    #[test]
    fn exe_suffix_is_dropped() {
        assert_eq!(short_name("Code.EXE"), "Code");
        assert_eq!(short_name("svchost.exe"), "svchost");
        assert_eq!(short_name("System"), "System");
        assert_eq!(short_name("가.exe"), "가");
        assert_eq!(short_name(".exe"), "");
    }
}
