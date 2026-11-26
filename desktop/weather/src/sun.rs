//! 해가 떠 있는지. 기상청은 낮과 밤을 주지 않아 좌표와 시각으로 계산한다(NOAA 근사식, 바깥 호출 없음).

use crate::time::Kst;

/// 해의 고도가 -0.833도(해 뜨고 지는 때)보다 높으면 낮
pub fn is_day(latitude: f64, longitude: f64, now: &Kst) -> bool {
    elevation(latitude, longitude, now) > -0.833
}

fn elevation(latitude: f64, longitude: f64, now: &Kst) -> f64 {
    let utc_hours = f64::from(now.hour) + f64::from(now.minute) / 60.0 - 9.0;
    let gamma = 2.0 * std::f64::consts::PI / 365.0
        * (f64::from(now.day_of_year()) - 1.0 + (utc_hours - 12.0) / 24.0);
    let equation = 229.18
        * (0.000075 + 0.001868 * gamma.cos()
            - 0.032077 * gamma.sin()
            - 0.014615 * (2.0 * gamma).cos()
            - 0.040849 * (2.0 * gamma).sin());
    let declination = 0.006918 - 0.399912 * gamma.cos() + 0.070257 * gamma.sin()
        - 0.006758 * (2.0 * gamma).cos()
        + 0.000907 * (2.0 * gamma).sin()
        - 0.002697 * (3.0 * gamma).cos()
        + 0.00148 * (3.0 * gamma).sin();
    let solar_minutes = utc_hours * 60.0 + equation + 4.0 * longitude;
    let hour_angle = (solar_minutes / 4.0 - 180.0).to_radians();
    let lat = latitude.to_radians();
    let cos_zenith =
        lat.sin() * declination.sin() + lat.cos() * declination.cos() * hour_angle.cos();
    90.0 - cos_zenith.clamp(-1.0, 1.0).acos().to_degrees()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::kst;

    #[test]
    fn seoul_in_late_november() {
        // 서울 11월 말 해돋이 약 07:25, 해넘이 약 17:15
        let day = |h, m| is_day(37.6, 127.0, &kst(2026, 11, 27, h, m));
        assert!(!day(7, 0));
        assert!(day(7, 50));
        assert!(day(12, 0));
        assert!(day(16, 50));
        assert!(!day(17, 40));
        assert!(!day(0, 30));
    }
}
