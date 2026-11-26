//! 윈도우 작업 개체. 자식이 띄운 손자(scoop shim 이 띄운 실제 gh 같은 것)까지 함께 끝낸다.

use std::os::windows::io::AsRawHandle;
use std::process::Child;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
};

/// 닫으면 안의 프로세스가 모두 끝나는 작업 개체
pub(crate) struct Job(HANDLE);

// SAFETY: 커널 핸들이라 어느 스레드에서 쓰고 닫아도 된다. Mutex 안에서만 쓴다.
unsafe impl Send for Job {}

impl Job {
    /// 자식을 넣은 작업 개체. 못 만들면 없음(그때는 자식만 끝낸다).
    /// 띄운 뒤 넣으므로 그 사이에 바로 띄운 손자는 빠질 수 있다.
    pub(crate) fn hold(child: &Child) -> Option<Self> {
        // SAFETY: 이름 없는 작업 개체. 실패하면 null.
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return None;
        }
        let job = Self(handle);
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: 우리 구조체와 그 크기
        let set = unsafe {
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        // SAFETY: 살아 있는 자식 핸들
        let assigned =
            set != 0 && unsafe { AssignProcessToJobObject(job.0, child.as_raw_handle()) } != 0;
        assigned.then_some(job)
    }

    pub(crate) fn kill(&self) {
        // SAFETY: 살아 있는 작업 개체
        unsafe { TerminateJobObject(self.0, 1) };
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: hold 에서 만든 핸들을 한 번만 닫는다. 닫으면 남은 손자도 끝난다.
        unsafe { CloseHandle(self.0) };
    }
}
