//! 한국 시간(UTC+9, 서머타임 없음) 날짜와 시각. 맥의 시간대와 상관없다.

use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Kst {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
}

impl Kst {
    pub fn from(time: SystemTime) -> Self {
        let seconds = time
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64)
            + 9 * 3600;
        Self::from_kst_seconds(seconds)
    }

    fn from_kst_seconds(seconds: i64) -> Self {
        let days = seconds.div_euclid(86_400);
        let rest = seconds.rem_euclid(86_400);
        let (year, month, day) = civil(days);
        Self {
            year,
            month,
            day,
            hour: (rest / 3600) as u32,
            minute: (rest % 3600 / 60) as u32,
        }
    }

    pub fn seconds(&self) -> i64 {
        days_from_civil(self.year, self.month, self.day) * 86_400
            + i64::from(self.hour) * 3600
            + i64::from(self.minute) * 60
    }

    /// 몇 시간 앞이나 뒤
    pub fn plus_hours(&self, hours: i64) -> Self {
        Self::from_kst_seconds(self.seconds() + hours * 3600)
    }

    /// "20261127"
    pub fn date(&self) -> String {
        format!("{:04}{:02}{:02}", self.year, self.month, self.day)
    }

    /// 1월 1일이 1
    pub fn day_of_year(&self) -> u32 {
        (days_from_civil(self.year, self.month, self.day) - days_from_civil(self.year, 1, 1)) as u32
            + 1
    }
}

/// 1970-01-01 부터 날 수 → (년, 월, 일). Howard Hinnant 의 식.
fn civil(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = (yoe + era * 400 + i64::from(month <= 2)) as i32;
    (year, month, day)
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let y = i64::from(year) - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let m = i64::from(month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
pub fn kst(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> Kst {
    Kst {
        year,
        month,
        day,
        hour,
        minute,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn utc_to_kst() {
        // 2026-11-26 15:30 UTC = 2026-11-27 00:30 KST
        let time = UNIX_EPOCH + Duration::from_secs(1_795_707_000);
        assert_eq!(Kst::from(time), kst(2026, 11, 27, 0, 30));
        assert_eq!(Kst::from(time).date(), "20261127");
    }

    #[test]
    fn hours_cross_days_and_years() {
        assert_eq!(
            kst(2027, 1, 1, 0, 5).plus_hours(-1),
            kst(2026, 12, 31, 23, 5)
        );
        assert_eq!(
            kst(2028, 2, 28, 23, 0).plus_hours(1),
            kst(2028, 2, 29, 0, 0)
        );
        assert_eq!(kst(2026, 12, 31, 12, 0).day_of_year(), 365);
    }
}
