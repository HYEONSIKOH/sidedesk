//! 날씨 설정: 고른 제공처는 설정 폴더의 `weather.txt`, 기상청 키는 운영체제 비밀 보관소.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

use sidedesk_domain::{Choice, KeyCheck, PortError, Secret, Secrets, WeatherSetup};
use sidedesk_weather::{Get, Kma, Settings};

pub struct WeatherPrefs {
    file: PathBuf,
    secrets: Box<dyn Secrets>,
    /// 날씨를 받을 수 없는 운영체제면 없다.
    kma: Option<Kma>,
    /// 시작 때 한 번 보고 넣기와 지우기에서 고친다. 스냅숏마다 보관소를 열지 않는다.
    has_key: AtomicBool,
}

impl WeatherPrefs {
    pub fn new(file: PathBuf, secrets: Box<dyn Secrets>, get: Option<Arc<dyn Get>>) -> Self {
        let has_key = get.is_some() && secrets.has_kma_key().unwrap_or(false);
        Self {
            file,
            secrets,
            kma: get.map(Kma::new),
            has_key: AtomicBool::new(has_key),
        }
    }
}

impl Settings for WeatherPrefs {
    fn choice(&self) -> Choice {
        match std::fs::read_to_string(&self.file)
            .as_deref()
            .map(str::trim)
        {
            Ok("kma") => Choice::Kma,
            _ => Choice::OpenMeteo,
        }
    }

    /// 보관소를 못 읽으면 키 없음과 같다.
    fn kma_key(&self) -> Option<Secret> {
        if !self.has_key.load(Ordering::Relaxed) {
            return None;
        }
        self.secrets.kma_key().ok().flatten()
    }
}

impl WeatherSetup for WeatherPrefs {
    fn supported(&self) -> bool {
        self.kma.is_some()
    }

    fn choice(&self) -> Choice {
        Settings::choice(self)
    }

    fn set_choice(&self, choice: Choice) -> Result<(), PortError> {
        let text = match choice {
            Choice::Kma => "kma",
            Choice::OpenMeteo => "open-meteo",
        };
        std::fs::write(&self.file, text).map_err(|_| PortError::new("설정을 저장하지 못했습니다"))
    }

    fn has_key(&self) -> bool {
        self.has_key.load(Ordering::Relaxed)
    }

    fn check_key(&self, key: &Secret) -> KeyCheck {
        self.kma
            .as_ref()
            .map_or(KeyCheck::Unknown, |kma| kma.check(key, SystemTime::now()))
    }

    fn save_key(&self, key: &Secret) -> Result<(), PortError> {
        self.secrets.set_kma_key(key)?;
        self.has_key.store(true, Ordering::Relaxed);
        Ok(())
    }

    fn clear_key(&self) -> Result<(), PortError> {
        self.secrets.clear_kma_key()?;
        self.has_key.store(false, Ordering::Relaxed);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Memory(Mutex<Option<String>>);

    impl Secrets for Memory {
        fn kma_key(&self) -> Result<Option<Secret>, PortError> {
            Ok(self.0.lock().unwrap().clone().map(Secret::new))
        }
        fn has_kma_key(&self) -> Result<bool, PortError> {
            Ok(self.0.lock().unwrap().is_some())
        }
        fn set_kma_key(&self, key: &Secret) -> Result<(), PortError> {
            *self.0.lock().unwrap() = Some(key.expose().to_owned());
            Ok(())
        }
        fn clear_kma_key(&self) -> Result<(), PortError> {
            *self.0.lock().unwrap() = None;
            Ok(())
        }
    }

    struct Nowhere;

    impl Get for Nowhere {
        fn get(&self, _url: &Secret) -> Result<sidedesk_weather::Response, PortError> {
            Err(PortError::new("연결 안 됨"))
        }
    }

    fn prefs(name: &str) -> WeatherPrefs {
        let file = std::env::temp_dir().join(format!("sidedesk-{name}-{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&file);
        WeatherPrefs::new(file, Box::new(Memory::default()), Some(Arc::new(Nowhere)))
    }

    #[test]
    fn choice_is_kept_in_a_file_and_defaults_to_open_meteo() {
        let prefs = prefs("choice");
        assert_eq!(Settings::choice(&prefs), Choice::OpenMeteo);
        prefs.set_choice(Choice::Kma).unwrap();
        assert_eq!(Settings::choice(&prefs), Choice::Kma);
        std::fs::write(&prefs.file, "odd").unwrap();
        assert_eq!(Settings::choice(&prefs), Choice::OpenMeteo);
        let _ = std::fs::remove_file(&prefs.file);
    }

    #[test]
    fn key_round_trip_and_offline_check() {
        let prefs = prefs("key");
        assert!(!prefs.has_key() && prefs.kma_key().is_none());
        assert_eq!(prefs.check_key(&Secret::new("k")), KeyCheck::Offline);
        prefs.save_key(&Secret::new("k")).unwrap();
        assert!(prefs.has_key());
        assert_eq!(prefs.kma_key().unwrap().expose(), "k");
        prefs.clear_key().unwrap();
        assert!(prefs.kma_key().is_none());
        // 설정 파일에는 제공처만 들어간다.
        prefs.set_choice(Choice::Kma).unwrap();
        assert_eq!(std::fs::read_to_string(&prefs.file).unwrap(), "kma");
        let _ = std::fs::remove_file(&prefs.file);
    }

    #[test]
    fn without_http_weather_is_unsupported() {
        let prefs = WeatherPrefs::new(
            PathBuf::from("/nonexistent"),
            Box::new(Memory::default()),
            None,
        );
        assert!(!prefs.supported());
        assert_eq!(prefs.check_key(&Secret::new("k")), KeyCheck::Unknown);
    }
}

/// 사람 키로 손으로 돌린다: `SIDEDESK_KMA_KEY_FILE=<키 파일> cargo test -p sidedesk real_kma -- --ignored`.
/// 설정 창의 키 넣기와 같은 길(확인 → 키체인 저장)을 거친 뒤 서울 시청 좌표로 받는다. 키는 찍지 않는다.
#[cfg(all(test, target_os = "macos"))]
mod real {
    use super::*;
    use sidedesk_core::Provider;
    use sidedesk_domain::{Place, WeatherSource};
    use sidedesk_mac::{MacGet, MacKeychain};
    use sidedesk_weather::Chooser;

    #[test]
    #[ignore = "기상청과 키체인을 실제로 쓴다. 손으로 돌린다"]
    fn real_kma() {
        let path = std::env::var("SIDEDESK_KMA_KEY_FILE").expect("키 파일 경로");
        let key = Secret::new(std::fs::read_to_string(path).expect("키 파일").trim());
        let get: Arc<dyn Get> = Arc::new(MacGet::default());
        let file = std::env::temp_dir().join("sidedesk-real-weather.txt");
        let prefs = Arc::new(WeatherPrefs::new(
            file.clone(),
            Box::new(MacKeychain::default()),
            Some(Arc::clone(&get)),
        ));
        let check = prefs.check_key(&key);
        eprintln!("키 확인: {check:?}");
        assert!(matches!(check, KeyCheck::Good | KeyCheck::Limit));
        prefs.save_key(&key).unwrap();
        prefs.set_choice(Choice::Kma).unwrap();
        let chooser = Chooser::new(prefs.clone(), get);
        let weather = chooser
            .fetch(Place {
                latitude: 376,
                longitude: 1270,
            })
            .unwrap();
        eprintln!(
            "받은 곳 {:?}, 기온 {}, 최고 {}, 최저 {}, 하늘 {:?}, 낮 {}",
            Provider::try_from(weather.provider),
            weather.temperature,
            weather.high,
            weather.low,
            sidedesk_core::Sky::try_from(weather.sky),
            weather.is_day
        );
        assert_eq!(weather.provider, Provider::Kma as i32);
        let _ = std::fs::remove_file(file);
    }
}
