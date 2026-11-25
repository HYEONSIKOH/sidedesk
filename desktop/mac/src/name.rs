use std::ptr;

use core_foundation::base::TCFType;
use core_foundation::string::CFString;
use sidedesk_domain::{ComputerName, PortError};
use system_configuration_sys::dynamic_store_copy_specific::SCDynamicStoreCopyComputerName;

pub struct MacName;

impl ComputerName for MacName {
    fn read(&self) -> Result<String, PortError> {
        // SAFETY: 두 인자는 null 을 허용하고, 돌려받은 문자열의 해제는 CFString 이 맡는다.
        let name = unsafe {
            let raw = SCDynamicStoreCopyComputerName(ptr::null(), ptr::null_mut());
            (!raw.is_null()).then(|| CFString::wrap_under_create_rule(raw))
        };
        name.map(|name| name.to_string())
            .ok_or_else(|| PortError::new("컴퓨터 이름을 읽지 못함"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_this_computer() {
        assert!(!MacName.read().unwrap().is_empty());
    }
}
