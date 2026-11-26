//! 대략적인 위치. 반올림과 범위 검사를 이 한 곳에서만 한다.

/// 위도, 경도를 x10 정수로. 반올림은 0 에서 먼 쪽.
/// NaN, 무한대, 범위 밖이면 위치 없음.
pub fn place_tenths(latitude: f64, longitude: f64) -> Option<(i32, i32)> {
    if !latitude.is_finite() || !longitude.is_finite() {
        return None;
    }
    let tenths = (
        (latitude * 10.0).round() as i32,
        (longitude * 10.0).round() as i32,
    );
    valid_tenths(tenths.0, tenths.1).then_some(tenths)
}

pub fn valid_tenths(latitude: i32, longitude: i32) -> bool {
    (-900..=900).contains(&latitude) && (-1800..=1800).contains(&longitude)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounds_half_away_from_zero() {
        assert_eq!(place_tenths(37.25, 127.0), Some((373, 1270)));
        assert_eq!(place_tenths(-0.25, -122.25), Some((-3, -1223)));
    }

    #[test]
    fn edges_and_garbage() {
        assert_eq!(place_tenths(90.0, -180.0), Some((900, -1800)));
        assert_eq!(place_tenths(-90.0, 180.0), Some((-900, 1800)));
        assert_eq!(place_tenths(90.1, 0.0), None);
        assert_eq!(place_tenths(0.0, 180.06), None);
        assert_eq!(place_tenths(f64::NAN, 0.0), None);
        assert_eq!(place_tenths(0.0, f64::INFINITY), None);
        assert!(!valid_tenths(901, 0));
    }
}
