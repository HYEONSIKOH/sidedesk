//! 날씨 받는 곳(어댑터). 운영체제를 모르고, HTTP 받기는 `Get` 으로 받는다.

mod chooser;
mod kma;
mod open_meteo;
mod sun;
mod time;

pub use chooser::{Chooser, Settings};
pub use kma::{Kma, grid};
pub use open_meteo::OpenMeteo;

use sidedesk_domain::{PortError, Secret};

/// 응답 하나. 본문은 상한 안에서 받는다.
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

/// HTTP GET. 통신 실패, 시간 넘음, 상한 넘음만 오류다. 상태 판정은 어댑터가 한다.
/// 주소에 키가 들 수 있어 `Secret` 으로 받는다. 오류 문장에 주소를 넣지 않는다.
pub trait Get: Send + Sync {
    fn get(&self, url: &Secret) -> Result<Response, PortError>;
}

fn failed() -> PortError {
    PortError::new("날씨를 받지 못했습니다")
}

/// x10 정수를 주소용 글자로. 0 쪽 음수의 부호를 잃지 않는다(-5 → "-0.5").
fn tenths(value: i16) -> String {
    let sign = if value < 0 { "-" } else { "" };
    let abs = value.unsigned_abs();
    format!("{sign}{}.{}", abs / 10, abs % 10)
}

/// 기온 검사: 숫자이고 -90~60 °C 안
fn temperature(value: f64) -> Option<f32> {
    (value.is_finite() && (-90.0..=60.0).contains(&value)).then_some(value as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tenths_keep_their_sign() {
        assert_eq!(tenths(-5), "-0.5");
        assert_eq!(tenths(-1234), "-123.4");
        assert_eq!(tenths(376), "37.6");
        assert_eq!(tenths(0), "0.0");
        assert_eq!(tenths(5), "0.5");
    }
}
