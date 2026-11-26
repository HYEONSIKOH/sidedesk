//! 블루투스 알림 신호에 싣는 값. 컴퓨터와 기기가 같은 코드로 만든다.

use sha2::{Digest, Sha256};

use crate::KEY_LEN;

/// SideDesk 블루투스 서비스 UUID. 알림 신호의 서비스 데이터에 쓴다.
pub const BLE_SERVICE: &str = "5d1d5e5c-7a3b-4c1e-9f20-0b1e5d35c001";
pub const BEACON_LEN: usize = 8;
/// 알림 신호 서비스 데이터: L2CAP PSM 2바이트(큰 끝) + 알림 신호 지문 8바이트.
pub const SERVICE_DATA_LEN: usize = 2 + BEACON_LEN;

/// 기기 공개 키로 만든 알림 신호 지문. 컴퓨터는 이 값으로 등록된 기기인지 붙기 전에 안다.
pub fn beacon(key: &[u8; KEY_LEN]) -> [u8; BEACON_LEN] {
    let digest = Sha256::new()
        .chain_update(b"sidedesk beacon")
        .chain_update(key)
        .finalize();
    let mut out = [0; BEACON_LEN];
    out.copy_from_slice(&digest[..BEACON_LEN]);
    out
}

/// 서비스 데이터를 PSM 과 알림 신호 지문으로 나눈다. 길이가 틀리면 None.
pub fn parse_service_data(data: &[u8]) -> Option<(u16, [u8; BEACON_LEN])> {
    if data.len() != SERVICE_DATA_LEN {
        return None;
    }
    let psm = u16::from_be_bytes([data[0], data[1]]);
    let mut mark = [0; BEACON_LEN];
    mark.copy_from_slice(&data[2..]);
    Some((psm, mark))
}

pub fn service_data(psm: u16, key: &[u8; KEY_LEN]) -> Vec<u8> {
    let mut data = psm.to_be_bytes().to_vec();
    data.extend_from_slice(&beacon(key));
    data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_data_round_trips() {
        let key = [9; KEY_LEN];
        let data = service_data(131, &key);
        assert_eq!(data.len(), SERVICE_DATA_LEN);
        assert_eq!(parse_service_data(&data), Some((131, beacon(&key))));
        assert_eq!(parse_service_data(&data[1..]), None);
    }

    #[test]
    fn different_keys_give_different_beacons() {
        assert_ne!(beacon(&[1; KEY_LEN]), beacon(&[2; KEY_LEN]));
    }
}
