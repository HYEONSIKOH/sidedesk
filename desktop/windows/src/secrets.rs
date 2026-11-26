//! 윈도우 자격 증명 관리자(일반 자격 증명). 같은 사용자의 다른 프로그램도 읽을 수 있다
//! (맥 키체인처럼 앱별 접근 목록이 없다). 막을 방법이 없어 위험으로 둔다.

use std::ptr;

use sidedesk_domain::{PortError, Secret, Secrets};
use windows_sys::Win32::Foundation::{ERROR_NOT_FOUND, GetLastError};
use windows_sys::Win32::Security::Credentials::{
    CRED_MAX_CREDENTIAL_BLOB_SIZE, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW,
    CredDeleteW, CredFree, CredReadW, CredWriteW,
};

use crate::handle::wide;

const PREFIX: &str = "io.github.hyeonsikoh.sidedesk/";
const KMA: &str = "kma";

pub struct WindowsSecrets {
    target: Vec<u16>,
}

impl Default for WindowsSecrets {
    fn default() -> Self {
        Self::account(KMA)
    }
}

impl WindowsSecrets {
    /// 시험은 다른 이름을 쓴다.
    pub fn account(account: &str) -> Self {
        Self {
            target: wide(&format!("{PREFIX}{account}")),
        }
    }

    /// 읽은 값을 건네고, 시스템이 준 버퍼는 0 으로 채운 뒤 놓는다. 없으면 None.
    fn read<T>(&self, take: impl FnOnce(&[u8]) -> T) -> Result<Option<T>, PortError> {
        let mut credential: *mut CREDENTIALW = ptr::null_mut();
        // SAFETY: 널 끝 이름. 성공하면 credential 은 CredFree 로 놓을 버퍼다.
        if unsafe { CredReadW(self.target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut credential) } == 0 {
            // SAFETY: 바로 앞 호출의 오류
            return match unsafe { GetLastError() } {
                ERROR_NOT_FOUND => Ok(None),
                _ => Err(broken("읽지")),
            };
        }
        struct Held(*mut CREDENTIALW);
        impl Drop for Held {
            fn drop(&mut self) {
                // SAFETY: CredReadW 가 준 버퍼. 값 자리를 0 으로 채우고 한 번만 놓는다.
                unsafe {
                    let credential = &*self.0;
                    if !credential.CredentialBlob.is_null() {
                        ptr::write_bytes(
                            credential.CredentialBlob,
                            0,
                            credential.CredentialBlobSize as usize,
                        );
                    }
                    CredFree(self.0.cast());
                }
            }
        }
        let held = Held(credential);
        // SAFETY: 성공한 CredReadW 의 결과
        let credential = unsafe { &*held.0 };
        let blob = if credential.CredentialBlob.is_null() {
            &[][..]
        } else {
            // SAFETY: 시스템이 준 값과 그 길이
            unsafe {
                std::slice::from_raw_parts(
                    credential.CredentialBlob,
                    credential.CredentialBlobSize as usize,
                )
            }
        };
        Ok(Some(take(blob)))
    }
}

fn broken(what: &str) -> PortError {
    // 문장에는 키 값도 오류 번호도 넣지 않는다.
    PortError::new(format!("자격 증명 관리자에서 {what} 못했습니다"))
}

impl Secrets for WindowsSecrets {
    fn kma_key(&self) -> Result<Option<Secret>, PortError> {
        match self.read(|blob| String::from_utf8(blob.to_vec()))? {
            Some(Ok(text)) => Ok(Some(Secret::new(text))),
            Some(Err(error)) => {
                // 잘못된 글자로 남은 사본도 지운다.
                let mut bytes = error.into_bytes();
                bytes.fill(0);
                Err(broken("읽지"))
            }
            None => Ok(None),
        }
    }

    fn has_kma_key(&self) -> Result<bool, PortError> {
        // 값을 꺼내지 않고 볼 방법이 없어 읽고 바로 지운다.
        Ok(self.read(|_| ())?.is_some())
    }

    fn set_kma_key(&self, key: &Secret) -> Result<(), PortError> {
        let mut blob = key.expose().as_bytes().to_vec();
        let result = self.write(&mut blob);
        blob.fill(0);
        result
    }

    fn clear_kma_key(&self) -> Result<(), PortError> {
        // SAFETY: 널 끝 이름
        if unsafe { CredDeleteW(self.target.as_ptr(), CRED_TYPE_GENERIC, 0) } != 0 {
            return Ok(());
        }
        // SAFETY: 바로 앞 호출의 오류
        match unsafe { GetLastError() } {
            ERROR_NOT_FOUND => Ok(()),
            _ => Err(broken("지우지")),
        }
    }
}

impl WindowsSecrets {
    fn write(&self, blob: &mut [u8]) -> Result<(), PortError> {
        if blob.is_empty() {
            return Err(PortError::new("빈 키는 저장하지 않습니다"));
        }
        if blob.len() > CRED_MAX_CREDENTIAL_BLOB_SIZE as usize {
            return Err(PortError::new("키가 너무 깁니다"));
        }
        let credential = CREDENTIALW {
            Type: CRED_TYPE_GENERIC,
            TargetName: self.target.as_ptr().cast_mut(),
            CredentialBlobSize: blob.len() as u32,
            CredentialBlob: blob.as_mut_ptr(),
            // 이 PC 의 이 사용자만. 다른 PC 로 옮겨 가지 않는다.
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            ..Default::default()
        };
        // SAFETY: 구조체의 포인터는 이 호출 동안 산다. 있으면 덮어쓴다.
        if unsafe { CredWriteW(&credential, 0) } == 0 {
            return Err(broken("저장하지"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "사용자 자격 증명 관리자에 시험 항목을 쓴다. CI 윈도우 작업에서 돌린다"]
    fn windows_only_put_read_and_clear() {
        let secrets = WindowsSecrets::account("sidedesk-test");
        secrets.clear_kma_key().unwrap();
        assert!(!secrets.has_kma_key().unwrap());
        assert!(secrets.kma_key().unwrap().is_none());
        secrets.set_kma_key(&Secret::new("first")).unwrap();
        secrets.set_kma_key(&Secret::new("둘째")).unwrap();
        assert!(secrets.has_kma_key().unwrap());
        assert_eq!(secrets.kma_key().unwrap().unwrap().expose(), "둘째");
        secrets.clear_kma_key().unwrap();
        secrets.clear_kma_key().unwrap();
        assert!(secrets.kma_key().unwrap().is_none());
    }

    #[test]
    fn empty_and_huge_keys_are_refused_before_saving() {
        let secrets = WindowsSecrets::account("sidedesk-never-written");
        assert!(secrets.set_kma_key(&Secret::new("")).is_err());
        let huge = "k".repeat(CRED_MAX_CREDENTIAL_BLOB_SIZE as usize + 1);
        assert!(secrets.set_kma_key(&Secret::new(huge)).is_err());
    }
}
