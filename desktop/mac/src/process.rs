//! 맥 프로세스 목록과 종료. 내 사용자 권한으로 읽을 수 있는 프로세스만 나온다.

use std::collections::HashMap;
use std::ffi::c_char;
use std::mem::{MaybeUninit, size_of};
use std::sync::Mutex;
use std::thread;
use std::time::Instant;

use sidedesk_core::Process;
use sidedesk_domain::{PortError, ProcessTable};

use crate::meter::{STALE, WARM_UP};

#[repr(C)]
struct Timebase {
    numer: u32,
    denom: u32,
}

unsafe extern "C" {
    fn mach_timebase_info(info: *mut Timebase) -> libc::c_int;
}

struct Read {
    pid: u32,
    name: String,
    started_at: u64,
    /// 나노초
    cpu_time: u64,
    memory: u64,
}

struct Previous {
    at: Instant,
    /// (pid, 시작 시각) → CPU 시간(나노초)
    cpu: HashMap<(u32, u64), u64>,
}

pub struct MacProcesses {
    /// mach 시각 단위를 나노초로 바꾸는 비
    timebase: (u64, u64),
    previous: Mutex<Option<Previous>>,
}

impl Default for MacProcesses {
    fn default() -> Self {
        let mut info = Timebase { numer: 1, denom: 1 };
        // SAFETY: info 를 채우는 호출
        unsafe { mach_timebase_info(&mut info) };
        Self {
            timebase: (u64::from(info.numer), u64::from(info.denom.max(1))),
            previous: Mutex::new(None),
        }
    }
}

impl ProcessTable for MacProcesses {
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
        let wall = at.duration_since(before.at).as_nanos().max(1) as f64;
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
        let gone = || PortError::new("프로세스가 이미 끝났거나 다른 프로세스로 바뀌었습니다");
        let pid = libc::pid_t::try_from(pid).map_err(|_| gone())?;
        let info = bsd_info(pid).ok_or_else(gone)?;
        if start_ms(&info) != started_at {
            return Err(gone());
        }
        // SAFETY: 신호만 보낸다.
        if unsafe { libc::kill(pid, libc::SIGTERM) } == 0 {
            return Ok(());
        }
        Err(match std::io::Error::last_os_error().raw_os_error() {
            Some(libc::ESRCH) => gone(),
            Some(libc::EPERM) => PortError::new("이 프로세스를 끝낼 권한이 없습니다"),
            _ => PortError::new("프로세스를 끝내지 못했습니다"),
        })
    }
}

impl MacProcesses {
    fn read_all(&self) -> Result<Vec<Read>, PortError> {
        Ok(pids()?
            .into_iter()
            .filter_map(|pid| self.read(pid))
            .collect())
    }

    fn read(&self, pid: libc::pid_t) -> Option<Read> {
        let info = bsd_info(pid)?;
        let mut usage = MaybeUninit::<libc::rusage_info_v2>::zeroed();
        // SAFETY: usage 는 rusage_info_v2 크기다. 남의 사용자 프로세스는 실패한다.
        let result =
            unsafe { libc::proc_pid_rusage(pid, libc::RUSAGE_INFO_V2, usage.as_mut_ptr().cast()) };
        if result != 0 {
            return None;
        }
        // SAFETY: 성공했으면 채워져 있다.
        let usage = unsafe { usage.assume_init() };
        let ticks = usage.ri_user_time + usage.ri_system_time;
        Some(Read {
            pid: pid as u32,
            name: name(&info),
            started_at: start_ms(&info),
            cpu_time: (u128::from(ticks) * u128::from(self.timebase.0)
                / u128::from(self.timebase.1)) as u64,
            memory: usage.ri_phys_footprint,
        })
    }
}

fn cpu_map(reads: &[Read]) -> HashMap<(u32, u64), u64> {
    reads
        .iter()
        .map(|read| ((read.pid, read.started_at), read.cpu_time))
        .collect()
}

fn pids() -> Result<Vec<libc::pid_t>, PortError> {
    // SAFETY: 개수만 묻는다.
    let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    if count <= 0 {
        return Err(PortError::new("프로세스 목록을 읽지 못함"));
    }
    // 그 사이 늘어난 프로세스를 담을 여유
    let mut pids = vec![0 as libc::pid_t; count as usize + 64];
    let bytes = (pids.len() * size_of::<libc::pid_t>()) as libc::c_int;
    // SAFETY: pids 는 bytes 바이트다.
    let count = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
    if count <= 0 {
        return Err(PortError::new("프로세스 목록을 읽지 못함"));
    }
    pids.truncate(count as usize);
    pids.retain(|pid| *pid > 0);
    Ok(pids)
}

fn bsd_info(pid: libc::pid_t) -> Option<libc::proc_bsdinfo> {
    let mut info = MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size = size_of::<libc::proc_bsdinfo>() as libc::c_int;
    // SAFETY: info 는 size 바이트다.
    let read = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    // SAFETY: 다 채웠을 때만 쓴다.
    (read == size).then(|| unsafe { info.assume_init() })
}

fn start_ms(info: &libc::proc_bsdinfo) -> u64 {
    info.pbi_start_tvsec * 1000 + info.pbi_start_tvusec / 1000
}

fn name(info: &libc::proc_bsdinfo) -> String {
    let text = |raw: &[c_char]| {
        let bytes: Vec<u8> = raw
            .iter()
            .take_while(|c| **c != 0)
            .map(|c| *c as u8)
            .collect();
        String::from_utf8_lossy(&bytes).into_owned()
    };
    let long = text(&info.pbi_name);
    if long.is_empty() {
        text(&info.pbi_comm)
    } else {
        long
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn lists_this_process_with_sane_values() {
        let table = MacProcesses::default();
        let list = table.processes().unwrap();
        let me = std::process::id();
        let mine = list.iter().find(|p| p.pid == me).expect("내 프로세스");
        assert!(!mine.name.is_empty());
        assert!(mine.memory > 0);
        assert!(mine.started_at > 1_600_000_000_000);
        let cores = thread::available_parallelism().map_or(64, |n| n.get()) as f32;
        assert!(
            list.iter()
                .all(|p| p.cpu >= 0.0 && p.cpu <= cores * 100.0 + 1.0)
        );
        assert!(list.len() > 10);
    }

    #[test]
    fn kills_only_when_the_start_time_matches() {
        let mut child = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        let pid = child.id();
        let table = MacProcesses::default();
        let started = table
            .processes()
            .unwrap()
            .into_iter()
            .find(|p| p.pid == pid)
            .expect("자식 프로세스")
            .started_at;
        assert!(table.kill(pid, started + 1).is_err());
        assert!(child.try_wait().unwrap().is_none());
        table.kill(pid, started).unwrap();
        let status = child.wait().unwrap();
        assert!(!status.success());
        assert!(table.kill(pid, started).is_err());
    }
}
