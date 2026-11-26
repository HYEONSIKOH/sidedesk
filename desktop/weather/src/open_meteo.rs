//! Open-Meteo(인증키 없음). 먼저 기상청 모델로 받고, 값이 비면 기본 모델로 다시 받는다.

use std::sync::Arc;

use serde_json::Value;
use sidedesk_core::{Provider, Sky, Weather};
use sidedesk_domain::{Place, PortError, Secret, WeatherSource};

use crate::{Get, failed, temperature, tenths};

const BASE: &str = "https://api.open-meteo.com/v1/forecast";
const KMA_MODEL: &str = "kma_seamless";

pub struct OpenMeteo {
    get: Arc<dyn Get>,
}

impl OpenMeteo {
    pub fn new(get: Arc<dyn Get>) -> Self {
        Self { get }
    }

    fn once(&self, place: Place, model: Option<&str>) -> Result<Read, PortError> {
        let response = self.get.get(&Secret::new(url(place, model)))?;
        if response.status != 200 {
            return Err(failed());
        }
        read(&response.body)
    }
}

impl WeatherSource for OpenMeteo {
    fn fetch(&self, place: Place) -> Result<Weather, PortError> {
        if let Ok(Read::Got(weather)) = self.once(place, Some(KMA_MODEL)) {
            return Ok(weather);
        }
        match self.once(place, None)? {
            Read::Got(weather) => Ok(weather),
            Read::Empty => Err(failed()),
        }
    }
}

fn url(place: Place, model: Option<&str>) -> String {
    let mut url = format!(
        "{BASE}?latitude={}&longitude={}&current=temperature_2m,weather_code,is_day\
         &daily=temperature_2m_max,temperature_2m_min&timezone=auto&forecast_days=1",
        tenths(place.latitude),
        tenths(place.longitude),
    );
    if let Some(model) = model {
        url.push_str("&models=");
        url.push_str(model);
    }
    url
}

#[derive(Debug)]
enum Read {
    Got(Weather),
    /// 필요한 값 가운데 null 이 있다(그 모델에 자료가 없음).
    Empty,
}

fn read(body: &[u8]) -> Result<Read, PortError> {
    let json: Value = serde_json::from_slice(body).map_err(|_| failed())?;
    let current = json.get("current").ok_or_else(failed)?;
    let daily = json.get("daily").ok_or_else(failed)?;
    let first = |name: &str| -> Result<&Value, PortError> {
        daily
            .get(name)
            .and_then(Value::as_array)
            .and_then(|values| values.first())
            .ok_or_else(failed)
    };
    let fields = [
        current.get("temperature_2m").ok_or_else(failed)?,
        current.get("weather_code").ok_or_else(failed)?,
        current.get("is_day").ok_or_else(failed)?,
        first("temperature_2m_max")?,
        first("temperature_2m_min")?,
    ];
    if fields.iter().any(|value| value.is_null()) {
        return Ok(Read::Empty);
    }
    let [now, code, day, high, low] = fields.map(Value::as_f64);
    let now = now.and_then(temperature).ok_or_else(failed)?;
    let high = high.and_then(temperature).ok_or_else(failed)?;
    let low = low.and_then(temperature).ok_or_else(failed)?;
    if high < low {
        return Err(failed());
    }
    let is_day = match day {
        Some(1.0) => true,
        Some(0.0) => false,
        _ => return Err(failed()),
    };
    let code = code
        .filter(|c| c.fract() == 0.0 && *c >= 0.0)
        .ok_or_else(failed)?;
    Ok(Read::Got(Weather {
        temperature: now,
        high,
        low,
        sky: wmo(code as u32) as i32,
        is_day,
        provider: Provider::OpenMeteo as i32,
        ..Default::default()
    }))
}

/// WMO 날씨 코드 → 하늘
fn wmo(code: u32) -> Sky {
    match code {
        0 => Sky::Clear,
        1 | 2 => Sky::PartlyCloudy,
        3 => Sky::Cloudy,
        45 | 48 => Sky::Fog,
        51..=57 => Sky::Drizzle,
        61..=67 => Sky::Rain,
        71..=77 | 85 | 86 => Sky::Snow,
        80..=82 => Sky::Shower,
        95..=99 => Sky::Thunderstorm,
        _ => Sky::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Response;
    use std::sync::Mutex;

    const SEOUL: Place = Place {
        latitude: 376,
        longitude: 1270,
    };

    fn body(now: &str, code: &str, day: &str, high: &str, low: &str) -> String {
        format!(
            r#"{{"latitude":37.6,"longitude":127.0,"current":{{"time":"2026-11-27T00:30","interval":900,"temperature_2m":{now},"weather_code":{code},"is_day":{day}}},"daily":{{"time":["2026-11-27"],"temperature_2m_max":[{high}],"temperature_2m_min":[{low}]}}}}"#
        )
    }

    /// 기상청 모델 응답과 기본 응답을 따로 주는 가짜
    struct Fake {
        kma: (u16, String),
        default: (u16, String),
        urls: Mutex<Vec<String>>,
    }

    impl Get for Fake {
        fn get(&self, url: &Secret) -> Result<Response, PortError> {
            let url = url.expose().to_owned();
            let (status, body) = if url.contains("models=kma_seamless") {
                &self.kma
            } else {
                &self.default
            };
            self.urls.lock().unwrap().push(url);
            Ok(Response {
                status: *status,
                body: body.clone().into_bytes(),
            })
        }
    }

    fn fetch(kma: (u16, String), default: (u16, String)) -> (Result<Weather, PortError>, usize) {
        let fake = Arc::new(Fake {
            kma,
            default,
            urls: Mutex::default(),
        });
        let result = OpenMeteo::new(fake.clone()).fetch(SEOUL);
        let calls = fake.urls.lock().unwrap().len();
        (result, calls)
    }

    #[test]
    fn reads_a_normal_answer() {
        let good = body("9.8", "3", "0", "14.2", "5.1");
        let (weather, calls) = fetch((200, good.clone()), (200, good));
        let weather = weather.unwrap();
        assert_eq!(calls, 1);
        assert_eq!(weather.temperature, 9.8);
        assert_eq!((weather.high, weather.low), (14.2, 5.1));
        assert_eq!(weather.sky, Sky::Cloudy as i32);
        assert!(!weather.is_day);
        assert_eq!(weather.provider, Provider::OpenMeteo as i32);
    }

    #[test]
    fn kma_model_nulls_fall_back_to_the_default_model() {
        let nulls = body("null", "null", "1", "null", "null");
        let good = body("9.8", "0", "1", "14.2", "5.1");
        let (weather, calls) = fetch((200, nulls.clone()), (200, good));
        assert_eq!(weather.unwrap().sky, Sky::Clear as i32);
        assert_eq!(calls, 2);
        // 최고만 null 이어도 기본으로 다시 받는다.
        let one_null = body("9.8", "0", "1", "null", "5.1");
        let (_, calls) = fetch((200, one_null), (200, body("1", "0", "1", "2", "1")));
        assert_eq!(calls, 2);
        // 둘 다 null 이면 실패
        let (weather, calls) = fetch((200, nulls.clone()), (200, nulls));
        assert!(weather.is_err());
        assert_eq!(calls, 2);
    }

    #[test]
    fn odd_answers_fail() {
        let good = body("9.8", "0", "1", "14.2", "5.1");
        for bad in [
            (400, r#"{"error":true,"reason":"x"}"#.to_owned()),
            (200, "not json".to_owned()),
            (200, body("99", "0", "1", "14.2", "5.1")),
            (200, body("9.8", "0", "2", "14.2", "5.1")),
            (200, body("9.8", "0", "1", "1", "5.1")),
            (200, body("9.8", "-1", "1", "14.2", "5.1")),
            (
                200,
                r#"{"current":{"temperature_2m":1,"weather_code":0,"is_day":1},"daily":{"temperature_2m_max":[],"temperature_2m_min":[]}}"#
                    .to_owned(),
            ),
        ] {
            let (weather, _) = fetch((200, body("null", "null", "1", "null", "null")), bad.clone());
            assert!(weather.is_err(), "{bad:?}");
        }
        let (weather, _) = fetch((500, String::new()), (200, good));
        assert!(weather.is_ok());
    }

    #[test]
    fn url_has_signed_tenths_and_no_key() {
        let url = url(
            Place {
                latitude: -5,
                longitude: -1234,
            },
            Some(KMA_MODEL),
        );
        assert!(url.contains("latitude=-0.5&longitude=-123.4"));
        assert!(url.ends_with("&models=kma_seamless"));
        assert!(!url.contains("apikey"));
    }

    #[test]
    fn wmo_codes() {
        assert_eq!(wmo(2), Sky::PartlyCloudy);
        assert_eq!(wmo(48), Sky::Fog);
        assert_eq!(wmo(56), Sky::Drizzle);
        assert_eq!(wmo(66), Sky::Rain);
        assert_eq!(wmo(86), Sky::Snow);
        assert_eq!(wmo(81), Sky::Shower);
        assert_eq!(wmo(96), Sky::Thunderstorm);
        assert_eq!(wmo(4), Sky::Unknown);
    }
}
