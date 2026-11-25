use std::ptr;

use sidedesk_domain::{ComputerName, PortError};
use windows_sys::Win32::System::SystemInformation::{
    ComputerNamePhysicalDnsHostname, GetComputerNameExW,
};

pub struct WindowsName;

impl ComputerName for WindowsName {
    fn read(&self) -> Result<String, PortError> {
        let mut length = 0;
        // SAFETY: 버퍼가 null 이면 필요한 길이만 채운다.
        unsafe {
            GetComputerNameExW(
                ComputerNamePhysicalDnsHostname,
                ptr::null_mut(),
                &mut length,
            );
        }
        let mut buffer = vec![0u16; length as usize];
        // SAFETY: 버퍼 길이를 그대로 넘긴다.
        let read = unsafe {
            GetComputerNameExW(
                ComputerNamePhysicalDnsHostname,
                buffer.as_mut_ptr(),
                &mut length,
            )
        };
        if read == 0 {
            return Err(PortError::new("컴퓨터 이름을 읽지 못함"));
        }
        Ok(String::from_utf16_lossy(&buffer[..length as usize]))
    }
}
