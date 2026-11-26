//! 아이폰과 맥이 같이 쓰는 값.

/// 아이폰 앱이 기기 안 127.0.0.1 에서 기다리는 포트. 맥은 usbmuxd 를 거쳐 여기로 붙는다.
pub const IOS_USB_PORT: u16 = 47_381;

/// 아이폰은 알림 신호에 서비스 데이터를 못 실어서, 같은 10바이트를 이 GATT 특성에 둔다.
pub const BLE_INFO: &str = "5d1d5e5c-7a3b-4c1e-9f20-0b1e5d35c002";

#[cfg(feature = "uniffi")]
#[uniffi::export]
pub fn usb_port() -> u16 {
    IOS_USB_PORT
}

#[cfg(feature = "uniffi")]
#[uniffi::export]
pub fn ble_info_uuid() -> String {
    BLE_INFO.to_owned()
}
