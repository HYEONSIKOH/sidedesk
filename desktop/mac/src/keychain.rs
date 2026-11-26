//! 맥 키체인(일반 암호 항목). Security 프레임워크를 직접 부른다.

use std::ptr;

use core_foundation::base::{CFType, CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::data::CFData;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::string::{CFString, CFStringRef};
use sidedesk_domain::{PortError, Secret, Secrets};

const SERVICE: &str = "io.github.hyeonsikoh.sidedesk";
const KMA: &str = "kma";
const SUCCESS: i32 = 0;
const NOT_FOUND: i32 = -25300;

#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    static kSecClass: CFStringRef;
    static kSecClassGenericPassword: CFStringRef;
    static kSecAttrService: CFStringRef;
    static kSecAttrAccount: CFStringRef;
    static kSecValueData: CFStringRef;
    static kSecReturnData: CFStringRef;
    fn SecItemAdd(attributes: CFDictionaryRef, result: *mut CFTypeRef) -> i32;
    fn SecItemCopyMatching(query: CFDictionaryRef, result: *mut CFTypeRef) -> i32;
    fn SecItemUpdate(query: CFDictionaryRef, attributes: CFDictionaryRef) -> i32;
    fn SecItemDelete(query: CFDictionaryRef) -> i32;
}

/// 키체인의 상수 이름
fn name(raw: CFStringRef) -> CFString {
    // SAFETY: Security 프레임워크가 주는 상수 문자열이다.
    unsafe { CFString::wrap_under_get_rule(raw) }
}

pub struct MacKeychain {
    account: String,
}

impl Default for MacKeychain {
    fn default() -> Self {
        Self::account(KMA)
    }
}

impl MacKeychain {
    /// 시험은 다른 계정 이름을 쓴다.
    pub fn account(account: &str) -> Self {
        Self {
            account: account.to_owned(),
        }
    }

    fn query(&self, extra: &[(CFString, CFType)]) -> CFDictionary<CFString, CFType> {
        // SAFETY: 위와 같다.
        let (class, generic, service, account) = unsafe {
            (
                name(kSecClass),
                name(kSecClassGenericPassword),
                name(kSecAttrService),
                name(kSecAttrAccount),
            )
        };
        let mut pairs = vec![
            (class, generic.as_CFType()),
            (service, CFString::new(SERVICE).as_CFType()),
            (account, CFString::new(&self.account).as_CFType()),
        ];
        pairs.extend(extra.iter().cloned());
        CFDictionary::from_CFType_pairs(&pairs)
    }
}

fn broken(what: &str) -> PortError {
    // 키체인 오류 번호만 남기고 값은 넣지 않는다.
    PortError::new(format!("키체인에서 {what} 못했습니다"))
}

impl Secrets for MacKeychain {
    fn kma_key(&self) -> Result<Option<Secret>, PortError> {
        // SAFETY: 상수 이름
        let returns = unsafe { name(kSecReturnData) };
        let query = self.query(&[(returns, CFBoolean::true_value().as_CFType())]);
        let mut result: CFTypeRef = ptr::null();
        // SAFETY: query 는 살아 있고, result 는 성공하면 우리가 놓을 CFData 다.
        let status = unsafe { SecItemCopyMatching(query.as_concrete_TypeRef(), &mut result) };
        match status {
            SUCCESS if !result.is_null() => {
                // SAFETY: 위에서 만든(copy) 값
                let data = unsafe { CFType::wrap_under_create_rule(result) };
                let data = data
                    .downcast_into::<CFData>()
                    .ok_or_else(|| broken("읽지"))?;
                let text = String::from_utf8(data.bytes().to_vec()).map_err(|_| broken("읽지"))?;
                Ok(Some(Secret::new(text)))
            }
            NOT_FOUND => Ok(None),
            _ => Err(broken("읽지")),
        }
    }

    fn has_kma_key(&self) -> Result<bool, PortError> {
        let query = self.query(&[]);
        // SAFETY: 값을 돌려받지 않는 물음
        let status = unsafe { SecItemCopyMatching(query.as_concrete_TypeRef(), ptr::null_mut()) };
        match status {
            SUCCESS => Ok(true),
            NOT_FOUND => Ok(false),
            _ => Err(broken("읽지")),
        }
    }

    fn set_kma_key(&self, key: &Secret) -> Result<(), PortError> {
        // SAFETY: 상수 이름
        let value = unsafe { name(kSecValueData) };
        let data = CFData::from_buffer(key.expose().as_bytes()).as_CFType();
        let change = CFDictionary::from_CFType_pairs(&[(value.clone(), data.clone())]);
        // SAFETY: 두 사전은 살아 있다.
        let status = unsafe {
            SecItemUpdate(
                self.query(&[]).as_concrete_TypeRef(),
                change.as_concrete_TypeRef(),
            )
        };
        if status == SUCCESS {
            return Ok(());
        }
        if status != NOT_FOUND {
            return Err(broken("저장하지"));
        }
        let add = self.query(&[(value, data)]);
        // SAFETY: 결과는 받지 않는다.
        let status = unsafe { SecItemAdd(add.as_concrete_TypeRef(), ptr::null_mut()) };
        (status == SUCCESS)
            .then_some(())
            .ok_or_else(|| broken("저장하지"))
    }

    fn clear_kma_key(&self) -> Result<(), PortError> {
        // SAFETY: query 는 살아 있다.
        let status = unsafe { SecItemDelete(self.query(&[]).as_concrete_TypeRef()) };
        match status {
            SUCCESS | NOT_FOUND => Ok(()),
            _ => Err(broken("지우지")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "로그인 키체인에 시험 항목을 쓴다. 손으로 돌린다"]
    fn put_read_and_clear() {
        let keychain = MacKeychain::account("sidedesk-test");
        keychain.clear_kma_key().unwrap();
        assert!(!keychain.has_kma_key().unwrap());
        keychain.set_kma_key(&Secret::new("first")).unwrap();
        keychain.set_kma_key(&Secret::new("second")).unwrap();
        assert!(keychain.has_kma_key().unwrap());
        assert_eq!(keychain.kma_key().unwrap().unwrap().expose(), "second");
        keychain.clear_kma_key().unwrap();
        assert!(keychain.kma_key().unwrap().is_none());
    }
}
