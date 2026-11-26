use sidedesk_core::{Performance, Process};
use sidedesk_domain::{Meter, PortError, ProcessTable};

fn unsupported() -> PortError {
    PortError::new("윈도우에서는 아직 지원하지 않습니다")
}

/// 윈도우 수집기는 다음 티켓에서 채운다.
pub struct WindowsMeter;

impl Meter for WindowsMeter {
    fn performance(&self) -> Result<Performance, PortError> {
        Err(unsupported())
    }
}

pub struct WindowsProcesses;

impl ProcessTable for WindowsProcesses {
    fn processes(&self) -> Result<Vec<Process>, PortError> {
        Err(unsupported())
    }

    fn kill(&self, _pid: u32, _started_at: u64) -> Result<(), PortError> {
        Err(unsupported())
    }
}
