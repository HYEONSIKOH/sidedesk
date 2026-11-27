//! 날씨 구독 장부. 좌표마다 한 번 받아 같은 좌표의 기기가 함께 쓴다.
//! 시각은 벽시계로 받는다(맥이 잠들면 `Instant` 가 멈춤).

use std::collections::{HashMap, HashSet};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sidedesk_core::{Location, Weather, valid_tenths};

use crate::port::{Place, PortError};

#[derive(Clone, Copy, Debug)]
pub struct WeatherCadence {
    /// 받은 뒤 다음 받기까지
    pub every: Duration,
    /// 실패가 이어질 때 간격. 끝 값을 계속 쓴다.
    pub retry: [Duration; 5],
    /// 할 일이 없어도 이만큼마다 깨어 벽시계와 정리를 본다.
    pub nap: Duration,
    /// 아무도 안 쓰는 좌표의 값을 이만큼 남긴다.
    pub keep: Duration,
}

impl Default for WeatherCadence {
    fn default() -> Self {
        let minutes = |m: u64| Duration::from_secs(60 * m);
        Self {
            every: minutes(15),
            retry: [minutes(1), minutes(2), minutes(4), minutes(8), minutes(15)],
            nap: minutes(1),
            keep: minutes(15),
        }
    }
}

impl Place {
    /// 둘 다 있고 범위 안일 때만 위치로 본다.
    pub fn from_location(location: &Location) -> Option<Self> {
        let (Some(latitude), Some(longitude)) =
            (location.latitude_tenths, location.longitude_tenths)
        else {
            return None;
        };
        valid_tenths(latitude, longitude).then_some(Self {
            latitude: latitude as i16,
            longitude: longitude as i16,
        })
    }
}

#[derive(Default)]
struct Want {
    wants: bool,
    place: Option<Place>,
}

impl Want {
    /// 컴퓨터 좌표(`here`)가 있으면 그것, 없으면 폰이 준 좌표
    fn place(&self, here: Option<Place>) -> Option<Place> {
        here.or(self.place).filter(|_| self.wants)
    }
}

struct Spot {
    latest: Option<Weather>,
    next: SystemTime,
    failures: u32,
    unused_since: Option<SystemTime>,
}

#[derive(Default)]
pub(crate) struct Book {
    wants: HashMap<u64, Want>,
    outbox: HashMap<u64, Weather>,
    spots: HashMap<Place, Spot>,
    /// 이 컴퓨터의 좌표. 있으면 모든 폰이 이 좌표로 받는다.
    here: Option<Place>,
}

impl Book {
    /// 구독이 바뀜. 값이 있으면 바로 보낼 칸에 넣는다. 받기 때를 당기지 않는다.
    pub fn set_wants(&mut self, id: u64, wants: bool) {
        self.wants.entry(id).or_default().wants = wants;
        self.offer(id);
    }

    pub fn set_place(&mut self, id: u64, place: Option<Place>) {
        self.wants.entry(id).or_default().place = place;
        self.offer(id);
    }

    /// 컴퓨터 좌표가 바뀜. 폰들에 다시 맞춘다.
    pub fn set_here(&mut self, here: Option<Place>) {
        if self.here == here {
            return;
        }
        self.here = here;
        let ids: Vec<u64> = self.wants.keys().copied().collect();
        for id in ids {
            self.offer(id);
        }
    }

    pub fn remove(&mut self, id: u64) {
        self.wants.remove(&id);
        self.outbox.remove(&id);
    }

    pub fn take(&mut self, id: u64) -> Option<Weather> {
        self.outbox.remove(&id)
    }

    fn offer(&mut self, id: u64) {
        let latest = self.wants[&id]
            .place(self.here)
            .and_then(|place| self.spots.get(&place))
            .and_then(|spot| spot.latest.clone());
        match latest {
            Some(weather) => self.outbox.insert(id, weather),
            None => self.outbox.remove(&id),
        };
    }

    fn wanted(&self) -> HashSet<Place> {
        self.wants.values().filter_map(|want| want.place(self.here)).collect()
    }

    /// 새 좌표는 바로 받게 하고, 안 쓰는 좌표는 시간이 지나면 지운다.
    pub fn tidy(&mut self, now: SystemTime, cadence: &WeatherCadence) {
        let wanted = self.wanted();
        for place in &wanted {
            self.spots.entry(*place).or_insert(Spot {
                latest: None,
                next: now,
                failures: 0,
                unused_since: None,
            });
        }
        self.spots.retain(|place, spot| {
            // 벽시계가 뒤로 가도 몇 시간씩 쉬지 않게 한다.
            if spot.next > now + cadence.every {
                spot.next = now + cadence.every;
            }
            if wanted.contains(place) {
                spot.unused_since = None;
                return true;
            }
            let since = *spot.unused_since.get_or_insert(now);
            elapsed(since, now) < cadence.keep
        });
    }

    /// 지금 받을 좌표
    pub fn due(&self, now: SystemTime) -> Option<Place> {
        self.wanted()
            .into_iter()
            .find(|place| self.spots.get(place).is_some_and(|spot| spot.next <= now))
    }

    /// 다음에 깰 때까지. 좌표가 하나도 없으면 없음(끝없이 잔다).
    pub fn wait(&self, now: SystemTime, cadence: &WeatherCadence) -> Option<Duration> {
        if self.spots.is_empty() {
            return None;
        }
        let wanted = self.wanted();
        let soonest = self
            .spots
            .iter()
            .filter(|(place, _)| wanted.contains(place))
            .map(|(_, spot)| spot.next.duration_since(now).unwrap_or(Duration::ZERO))
            .min()
            .unwrap_or(cadence.nap);
        Some(soonest.min(cadence.nap))
    }

    pub fn finish(
        &mut self,
        place: Place,
        result: Result<Weather, PortError>,
        now: SystemTime,
        cadence: &WeatherCadence,
    ) {
        let Some(spot) = self.spots.get_mut(&place) else {
            return;
        };
        match result {
            Ok(mut weather) => {
                weather.fetched_at = millis(now);
                weather.latitude_tenths = i32::from(place.latitude);
                weather.longitude_tenths = i32::from(place.longitude);
                spot.latest = Some(weather.clone());
                spot.failures = 0;
                spot.next = now + cadence.every;
                for (id, want) in &self.wants {
                    if want.place(self.here) == Some(place) {
                        self.outbox.insert(*id, weather.clone());
                    }
                }
            }
            Err(_) => {
                spot.failures += 1;
                let step = (spot.failures as usize).min(cadence.retry.len()) - 1;
                spot.next = now + cadence.retry[step];
            }
        }
    }
}

fn elapsed(since: SystemTime, now: SystemTime) -> Duration {
    now.duration_since(since).unwrap_or(Duration::ZERO)
}

fn millis(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEOUL: Place = Place {
        latitude: 376,
        longitude: 1270,
    };
    const BUSAN: Place = Place {
        latitude: 352,
        longitude: 1291,
    };

    fn at(minutes: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_800_000_000 + minutes * 60)
    }

    fn sunny() -> Weather {
        Weather {
            temperature: 10.0,
            ..Default::default()
        }
    }

    fn fail() -> Result<Weather, PortError> {
        Err(PortError::new("실패"))
    }

    #[test]
    fn needs_both_a_subscription_and_a_place() {
        let c = WeatherCadence::default();
        let mut book = Book::default();
        book.set_wants(1, true);
        book.tidy(at(0), &c);
        assert_eq!(book.due(at(0)), None);
        assert_eq!(book.wait(at(0), &c), None);
        book.set_wants(1, false);
        book.set_place(1, Some(SEOUL));
        book.tidy(at(0), &c);
        assert_eq!(book.due(at(0)), None);
        // 위치가 먼저 와도 구독이 오면 받는다.
        book.set_wants(1, true);
        book.tidy(at(0), &c);
        assert_eq!(book.due(at(0)), Some(SEOUL));
    }

    #[test]
    fn the_computer_place_wins_over_the_phones() {
        let c = WeatherCadence::default();
        let mut book = Book::default();
        book.set_wants(1, true);
        book.set_wants(2, true);
        book.set_place(2, Some(BUSAN));
        book.set_here(Some(SEOUL));
        book.tidy(at(0), &c);
        assert_eq!(book.wanted(), [SEOUL].into_iter().collect(), "폰 위치와 상관없이 컴퓨터 위치");
        book.set_here(None);
        book.tidy(at(0), &c);
        assert_eq!(book.wanted(), [BUSAN].into_iter().collect(), "컴퓨터 위치를 모르면 폰 위치");
    }

    #[test]
    fn one_fetch_per_place_shared_by_phones() {
        let c = WeatherCadence::default();
        let mut book = Book::default();
        for id in [1, 2] {
            book.set_wants(id, true);
            book.set_place(id, Some(SEOUL));
        }
        book.set_wants(3, true);
        book.set_place(3, Some(BUSAN));
        book.tidy(at(0), &c);
        let mut due = Vec::new();
        while let Some(place) = book.due(at(0)) {
            due.push(place);
            book.finish(place, Ok(sunny()), at(0), &c);
        }
        assert_eq!(due.len(), 2);
        assert!(due.contains(&SEOUL) && due.contains(&BUSAN));
        let one = book.take(1).unwrap();
        assert_eq!((one.latitude_tenths, one.longitude_tenths), (376, 1270));
        assert!(book.take(2).is_some());
        assert_eq!(book.take(3).unwrap().latitude_tenths, 352);
        assert_eq!(book.due(at(14)), None);
        assert!(book.due(at(15)).is_some());
    }

    #[test]
    fn failures_back_off_and_reconnects_do_not_pull_it_in() {
        let c = WeatherCadence::default();
        let mut book = Book::default();
        book.set_wants(1, true);
        book.set_place(1, Some(SEOUL));
        book.tidy(at(0), &c);
        let mut tries = Vec::new();
        for minute in 0..60 {
            // 매분 끊겼다 다시 붙는다.
            book.remove(1);
            book.set_wants(1, true);
            book.set_place(1, Some(SEOUL));
            book.tidy(at(minute), &c);
            if book.due(at(minute)).is_some() {
                tries.push(minute);
                book.finish(SEOUL, fail(), at(minute), &c);
            }
            assert!(book.take(1).is_none());
        }
        assert_eq!(tries, [0, 1, 3, 7, 15, 30, 45]);
    }

    #[test]
    fn a_kept_value_goes_out_at_once_even_when_old() {
        let c = WeatherCadence::default();
        let mut book = Book::default();
        book.set_wants(1, true);
        book.set_place(1, Some(SEOUL));
        book.tidy(at(0), &c);
        book.finish(SEOUL, Ok(sunny()), at(0), &c);
        book.remove(1);
        book.tidy(at(1), &c);
        // 14분 뒤 다시 붙음: 받아 둔 값을 바로 준다. 받기는 15분에.
        book.set_wants(2, true);
        book.set_place(2, Some(SEOUL));
        assert!(book.take(2).is_some());
        book.tidy(at(14), &c);
        assert_eq!(book.due(at(14)), None);
        assert_eq!(book.due(at(15)), Some(SEOUL));
    }

    #[test]
    fn no_place_stops_only_that_phone() {
        let c = WeatherCadence::default();
        let mut book = Book::default();
        for id in [1, 2] {
            book.set_wants(id, true);
            book.set_place(id, Some(SEOUL));
        }
        book.tidy(at(0), &c);
        book.finish(SEOUL, Ok(sunny()), at(0), &c);
        book.set_place(1, None);
        assert!(book.take(1).is_none());
        assert!(book.take(2).is_some());
        book.tidy(at(15), &c);
        book.finish(SEOUL, Ok(sunny()), at(15), &c);
        assert!(book.take(1).is_none());
        assert!(book.take(2).is_some());
    }

    #[test]
    fn unused_places_are_forgotten_and_the_thread_can_sleep() {
        let c = WeatherCadence::default();
        let mut book = Book::default();
        book.set_wants(1, true);
        book.set_place(1, Some(SEOUL));
        book.tidy(at(0), &c);
        book.finish(SEOUL, Ok(sunny()), at(0), &c);
        book.remove(1);
        book.tidy(at(1), &c);
        assert_eq!(book.wait(at(1), &c), Some(c.nap));
        book.tidy(at(16), &c);
        assert_eq!(book.wait(at(16), &c), None);
        // 지운 뒤 다시 오면 새 좌표로 바로 받는다.
        book.set_wants(2, true);
        book.set_place(2, Some(SEOUL));
        assert!(book.take(2).is_none());
        book.tidy(at(17), &c);
        assert_eq!(book.due(at(17)), Some(SEOUL));
    }

    #[test]
    fn a_clock_set_back_does_not_stall_fetching() {
        let c = WeatherCadence::default();
        let mut book = Book::default();
        book.set_wants(1, true);
        book.set_place(1, Some(SEOUL));
        book.tidy(at(600), &c);
        book.finish(SEOUL, Ok(sunny()), at(600), &c);
        // 벽시계가 10시간 뒤로 감
        book.tidy(at(0), &c);
        assert_eq!(book.due(at(14)), None);
        assert_eq!(book.due(at(15)), Some(SEOUL));
    }

    #[test]
    fn locations_need_both_values_in_range() {
        let both = |a, b| Location {
            latitude_tenths: a,
            longitude_tenths: b,
        };
        assert_eq!(
            Place::from_location(&both(Some(376), Some(1270))),
            Some(SEOUL)
        );
        assert_eq!(Place::from_location(&both(Some(376), None)), None);
        assert_eq!(Place::from_location(&both(Some(901), Some(0))), None);
        assert_eq!(Place::from_location(&Location::default()), None);
    }
}
