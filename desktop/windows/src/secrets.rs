use sidedesk_domain::{PortError, Secret, Secrets};

fn unsupported() -> PortError {
    PortError::new("윈도우에서는 아직 지원하지 않습니다")
}

/// 자격 증명 관리자 자리. 다음 티켓에서 채운다.
pub struct WindowsSecrets;

impl Secrets for WindowsSecrets {
    fn kma_key(&self) -> Result<Option<Secret>, PortError> {
        Ok(None)
    }

    fn has_kma_key(&self) -> Result<bool, PortError> {
        Ok(false)
    }

    fn set_kma_key(&self, _key: &Secret) -> Result<(), PortError> {
        Err(unsupported())
    }

    fn clear_kma_key(&self) -> Result<(), PortError> {
        Err(unsupported())
    }
}
