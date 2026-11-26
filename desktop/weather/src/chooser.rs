//! 사람이 고른 제공처를 따르되, 기상청을 못 쓰면 그 회차는 Open-Meteo 로 받는다.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use sidedesk_core::Weather;
use sidedesk_domain::{Choice, Place, PortError, Secret, WeatherSource};

use crate::{Get, Kma, OpenMeteo};

/// 기상청이 이만큼 이어서 실패하면 잠시 쉰다.
const KMA_FAILURES: u32 = 3;
const KMA_REST: Duration = Duration::from_secs(30 * 60);

/// 받을 때마다 다시 읽는 설정
pub trait Settings: Send + Sync {
    fn choice(&self) -> Choice;
    fn kma_key(&self) -> Option<Secret>;
}

#[derive(Default)]
struct Health {
    failures: u32,
    resting_until: Option<Instant>,
    /// 키가 바뀌었는지 보려고 키의 해시만 든다.
    key: Option<u64>,
}

pub struct Chooser {
    settings: Arc<dyn Settings>,
    kma: Kma,
    open_meteo: OpenMeteo,
    health: Mutex<Health>,
}

impl Chooser {
    pub fn new(settings: Arc<dyn Settings>, get: Arc<dyn Get>) -> Self {
        Self {
            settings,
            kma: Kma::new(Arc::clone(&get)),
            open_meteo: OpenMeteo::new(get),
            health: Mutex::default(),
        }
    }

    fn try_kma(&self, place: Place) -> Option<Weather> {
        if self.settings.choice() != Choice::Kma {
            return None;
        }
        let key = self.settings.kma_key()?;
        let mut hasher = DefaultHasher::new();
        key.expose().hash(&mut hasher);
        let hash = hasher.finish();
        {
            let mut health = self.lock();
            if health.key != Some(hash) {
                *health = Health {
                    key: Some(hash),
                    ..Health::default()
                };
            }
            if health
                .resting_until
                .is_some_and(|until| Instant::now() < until)
            {
                return None;
            }
        }
        let result = self.kma.fetch(place, &key, SystemTime::now());
        let mut health = self.lock();
        match result {
            Ok(weather) => {
                health.failures = 0;
                health.resting_until = None;
                Some(weather)
            }
            Err(_) => {
                health.failures += 1;
                if health.failures >= KMA_FAILURES {
                    health.failures = 0;
                    health.resting_until = Some(Instant::now() + KMA_REST);
                }
                None
            }
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Health> {
        self.health.lock().unwrap_or_else(|lock| lock.into_inner())
    }
}

impl WeatherSource for Chooser {
    fn fetch(&self, place: Place) -> Result<Weather, PortError> {
        match self.try_kma(place) {
            Some(weather) => Ok(weather),
            None => self.open_meteo.fetch(place),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Response;
    use sidedesk_core::Provider;

    struct Fixed {
        choice: Choice,
        key: Mutex<Option<&'static str>>,
    }

    impl Settings for Fixed {
        fn choice(&self) -> Choice {
            self.choice
        }

        fn kma_key(&self) -> Option<Secret> {
            self.key.lock().unwrap().map(Secret::new)
        }
    }

    /// 기상청은 늘 실패(500), Open-Meteo 는 성공
    #[derive(Default)]
    struct Count {
        kma: Mutex<u32>,
        open_meteo: Mutex<u32>,
    }

    impl Get for Count {
        fn get(&self, url: &Secret) -> Result<Response, PortError> {
            if url.expose().contains("apis.data.go.kr") {
                *self.kma.lock().unwrap() += 1;
                return Ok(Response {
                    status: 500,
                    body: Vec::new(),
                });
            }
            *self.open_meteo.lock().unwrap() += 1;
            Ok(Response {
                status: 200,
                body: br#"{"current":{"temperature_2m":1.0,"weather_code":0,"is_day":1},"daily":{"temperature_2m_max":[3.0],"temperature_2m_min":[0.0]}}"#.to_vec(),
            })
        }
    }

    const SEOUL: Place = Place {
        latitude: 376,
        longitude: 1270,
    };

    fn chooser(choice: Choice, key: Option<&'static str>) -> (Chooser, Arc<Count>, Arc<Fixed>) {
        let get = Arc::new(Count::default());
        let settings = Arc::new(Fixed {
            choice,
            key: Mutex::new(key),
        });
        (Chooser::new(settings.clone(), get.clone()), get, settings)
    }

    #[test]
    fn open_meteo_choice_never_calls_kma() {
        let (chooser, get, _) = chooser(Choice::OpenMeteo, Some("k"));
        let weather = chooser.fetch(SEOUL).unwrap();
        assert_eq!(weather.provider, Provider::OpenMeteo as i32);
        assert_eq!(*get.kma.lock().unwrap(), 0);
    }

    #[test]
    fn kma_without_a_key_uses_open_meteo() {
        let (chooser, get, _) = chooser(Choice::Kma, None);
        assert!(chooser.fetch(SEOUL).is_ok());
        assert_eq!(*get.kma.lock().unwrap(), 0);
    }

    #[test]
    fn kma_failures_fall_back_and_then_rest() {
        let (chooser, get, settings) = chooser(Choice::Kma, Some("k"));
        for _ in 0..3 {
            let weather = chooser.fetch(SEOUL).unwrap();
            assert_eq!(weather.provider, Provider::OpenMeteo as i32);
        }
        assert_eq!(*get.kma.lock().unwrap(), 3);
        // 세 번 실패 뒤에는 쉰다.
        chooser.fetch(SEOUL).unwrap();
        assert_eq!(*get.kma.lock().unwrap(), 3);
        // 키를 바꾸면 다시 해 본다.
        *settings.key.lock().unwrap() = Some("new");
        chooser.fetch(SEOUL).unwrap();
        assert_eq!(*get.kma.lock().unwrap(), 4);
    }
}
