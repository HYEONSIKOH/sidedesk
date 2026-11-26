//! 기상청 단기예보 조회서비스(공공데이터포털).
//! 지금 기온과 강수형태는 초단기실황, 하늘은 단기예보, 오늘 최고와 최저는 하루 한 번 받는 이른 발표에서.
//! 2026-11-27 사용자 키로 실제로 불러 동작 이름, 응답 겹침, 값이 글자로 옴, 02시 발표의 TMN/TMX,
//! 잘못된 키 응답(HTTP 403 + returnReasonCode 30), 이른 base_time 응답(resultCode 10)을 확인했다.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use serde_json::Value;
use sidedesk_core::{Provider, Sky, Weather};
use sidedesk_domain::{KeyCheck, Place, PortError, Secret};

use crate::time::Kst;
use crate::{Get, failed, sun, temperature};

const BASE: &str = "https://apis.data.go.kr/1360000/VilageFcstInfoService_2.0";
const NOWCAST: &str = "getUltraSrtNcst";
const FORECAST: &str = "getVilageFcst";
/// 초단기실황은 매시 이 분 이후에 그 시각 것을 부른다.
const NOWCAST_READY: u32 = 40;
/// 단기예보 발표 시각과, 발표 뒤 부를 수 있는 분
const FORECAST_HOURS: [u32; 8] = [2, 5, 8, 11, 14, 17, 20, 23];
const FORECAST_READY: u32 = 10;
/// 지금 쓸 하늘은 앞쪽 몇 시간만 있으면 된다(한 시각에 항목 12개쯤).
const FORECAST_ROWS: u32 = 100;
/// 이른 발표에서 오늘 15시 TMX 까지 덮는다.
const DAILY_ROWS: u32 = 400;
/// 결측값(절댓값이 이보다 크면 없음)
const MISSING: f64 = 900.0;
/// 확인용 고정 격자(서울 시청)
const CHECK_GRID: (i32, i32) = (60, 127);
/// 게이트웨이 오류 코드(returnReasonCode)
const KEY_WRONG: [&str; 4] = ["20", "30", "31", "32"];
const KEY_LIMIT: &str = "22";

struct Forecast {
    base: String,
    /// (예보 날짜와 시각 "202611270300", SKY, PTY)
    slots: Vec<(String, Option<i64>, Option<i64>)>,
}

struct Daily {
    date: String,
    low: f32,
    high: f32,
}

#[derive(Default)]
struct Cache {
    forecast: HashMap<(i32, i32), Forecast>,
    daily: HashMap<(i32, i32), Daily>,
}

pub struct Kma {
    get: Arc<dyn Get>,
    cache: Mutex<Cache>,
}

impl Kma {
    pub fn new(get: Arc<dyn Get>) -> Self {
        Self {
            get,
            cache: Mutex::default(),
        }
    }

    /// 국내 격자가 아니면 바로 실패한다.
    pub fn fetch(&self, place: Place, key: &Secret, now: SystemTime) -> Result<Weather, PortError> {
        let latitude = f64::from(place.latitude) / 10.0;
        let longitude = f64::from(place.longitude) / 10.0;
        let cell = grid(latitude, longitude);
        if !in_korea(cell) {
            return Err(failed());
        }
        let kst = Kst::from(now);
        let (temperature, now_pty) = self.nowcast(key, cell, &kst)?;
        let (sky, pty) = self.sky(key, cell, &kst)?;
        let (low, high) = self.daily(key, cell, &kst)?;
        let sky = match (now_pty, pty) {
            (Some(code), _) if code != 0 => nowcast_rain(code),
            (_, Some(code)) if code != 0 => forecast_rain(code),
            _ => sky.map_or(Sky::Unknown, sky_code),
        };
        if sky == Sky::Unknown {
            return Err(failed());
        }
        Ok(Weather {
            temperature,
            high,
            low,
            sky: sky as i32,
            is_day: sun::is_day(latitude, longitude, &kst),
            provider: Provider::Kma as i32,
            ..Default::default()
        })
    }

    /// 설정 창에서 키를 넣을 때 한 번 불러 본다. 좌표는 기기 위치가 아닌 고정 격자.
    pub fn check(&self, key: &Secret, now: SystemTime) -> KeyCheck {
        let base = nowcast_base(&Kst::from(now));
        let url = url(NOWCAST, key, &base, 10, CHECK_GRID);
        let Ok(response) = self.get.get(&url) else {
            return KeyCheck::Offline;
        };
        if let Some(code) = gateway_code(&response.body) {
            return if KEY_WRONG.contains(&code.as_str()) {
                KeyCheck::Wrong
            } else if code == KEY_LIMIT {
                KeyCheck::Limit
            } else {
                KeyCheck::Unknown
            };
        }
        if response.status == 429 {
            return KeyCheck::Limit;
        }
        // 기상청 본문의 resultCode 까지 왔으면 키는 게이트웨이를 지났다.
        match result_code(&response.body) {
            Some(_) if response.status == 200 => KeyCheck::Good,
            _ => KeyCheck::Unknown,
        }
    }

    fn call(
        &self,
        operation: &str,
        key: &Secret,
        base: &Kst,
        rows: u32,
        cell: (i32, i32),
    ) -> Result<Vec<Value>, PortError> {
        let response = self.get.get(&url(operation, key, base, rows, cell))?;
        if response.status != 200 {
            return Err(failed());
        }
        items(&response.body)
    }

    fn nowcast(
        &self,
        key: &Secret,
        cell: (i32, i32),
        kst: &Kst,
    ) -> Result<(f32, Option<i64>), PortError> {
        let items = self.call(NOWCAST, key, &nowcast_base(kst), 10, cell)?;
        let find = |category| {
            items
                .iter()
                .find(|item| item["category"] == category)
                .and_then(|item| number(&item["obsrValue"]))
        };
        let now = find("T1H").and_then(temperature).ok_or_else(failed)?;
        Ok((now, find("PTY").map(|code| code as i64)))
    }

    /// 지금 시각에 가장 가까운 예보 칸의 SKY, PTY. 같은 발표면 기억한 것을 쓴다.
    fn sky(
        &self,
        key: &Secret,
        cell: (i32, i32),
        kst: &Kst,
    ) -> Result<(Option<i64>, Option<i64>), PortError> {
        let base = forecast_base(kst);
        let stamp = format!("{}{:02}00", base.date(), base.hour);
        let mut cache = self.lock();
        if cache.forecast.get(&cell).is_none_or(|f| f.base != stamp) {
            drop(cache);
            let items = self.call(FORECAST, key, &base, FORECAST_ROWS, cell)?;
            let forecast = Forecast {
                base: stamp,
                slots: slots(&items),
            };
            cache = self.lock();
            cache.forecast.insert(cell, forecast);
        }
        let hour = format!("{}{:02}00", kst.date(), kst.hour);
        cache.forecast[&cell]
            .slots
            .iter()
            .find(|(at, _, _)| *at >= hour)
            .map(|(_, sky, pty)| (*sky, *pty))
            .ok_or_else(failed)
    }

    /// 오늘 최고와 최저. 발표마다 들어 있지 않아 이른 발표를 하루 한 번 받는다.
    fn daily(&self, key: &Secret, cell: (i32, i32), kst: &Kst) -> Result<(f32, f32), PortError> {
        let today = kst.date();
        {
            let mut cache = self.lock();
            cache.daily.retain(|_, daily| daily.date == today);
            if let Some(daily) = cache.daily.get(&cell) {
                return Ok((daily.low, daily.high));
            }
        }
        let items = self.call(FORECAST, key, &daily_base(kst), DAILY_ROWS, cell)?;
        let find = |category| {
            items
                .iter()
                .find(|item| item["category"] == category && item["fcstDate"] == today.as_str())
                .and_then(|item| number(&item["fcstValue"]))
                .and_then(temperature)
        };
        let (Some(low), Some(high)) = (find("TMN"), find("TMX")) else {
            return Err(failed());
        };
        if high < low {
            return Err(failed());
        }
        self.lock().daily.insert(
            cell,
            Daily {
                date: today,
                low,
                high,
            },
        );
        Ok((low, high))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Cache> {
        self.cache.lock().unwrap_or_else(|lock| lock.into_inner())
    }
}

/// 위경도 → 기상청 5km 격자(람베르트 정각원추도법). 상수는 기상청 격자 기준값.
pub fn grid(latitude: f64, longitude: f64) -> (i32, i32) {
    use std::f64::consts::PI;
    const EARTH: f64 = 6371.00877;
    const CELL: f64 = 5.0;
    const SLAT1: f64 = 30.0;
    const SLAT2: f64 = 60.0;
    const OLON: f64 = 126.0;
    const OLAT: f64 = 38.0;
    const XO: f64 = 43.0;
    const YO: f64 = 136.0;
    let rad = PI / 180.0;
    let re = EARTH / CELL;
    let (slat1, slat2, olon, olat) = (SLAT1 * rad, SLAT2 * rad, OLON * rad, OLAT * rad);
    let quarter = |angle: f64| (PI * 0.25 + angle * 0.5).tan();
    let sn = (slat1.cos() / slat2.cos()).ln() / (quarter(slat2) / quarter(slat1)).ln();
    let sf = quarter(slat1).powf(sn) * slat1.cos() / sn;
    let ro = re * sf / quarter(olat).powf(sn);
    let ra = re * sf / quarter(latitude * rad).powf(sn);
    let mut theta = longitude * rad - olon;
    if theta > PI {
        theta -= 2.0 * PI;
    }
    if theta < -PI {
        theta += 2.0 * PI;
    }
    theta *= sn;
    let x = (ra * theta.sin() + XO + 0.5).floor();
    let y = (ro - ra * theta.cos() + YO + 0.5).floor();
    (x as i32, y as i32)
}

fn in_korea((x, y): (i32, i32)) -> bool {
    (1..=149).contains(&x) && (1..=253).contains(&y)
}

/// 매시 40분 이후면 그 시각, 아니면 한 시간 전
fn nowcast_base(kst: &Kst) -> Kst {
    let base = if kst.minute >= NOWCAST_READY {
        *kst
    } else {
        kst.plus_hours(-1)
    };
    Kst { minute: 0, ..base }
}

/// 발표 10분이 지난 가장 최근 단기예보. 02:10 전이면 어제 23시.
fn forecast_base(kst: &Kst) -> Kst {
    let minutes = kst.hour * 60 + kst.minute;
    match FORECAST_HOURS
        .iter()
        .rev()
        .find(|hour| minutes >= *hour * 60 + FORECAST_READY)
    {
        Some(hour) => Kst {
            hour: *hour,
            minute: 0,
            ..*kst
        },
        None => Kst {
            hour: 23,
            minute: 0,
            ..kst.plus_hours(-24)
        },
    }
}

/// 오늘 TMN, TMX 가 모두 든 이른 발표: 오늘 02시, 02:10 전이면 어제 23시
fn daily_base(kst: &Kst) -> Kst {
    let early = Kst {
        hour: 2,
        minute: 0,
        ..*kst
    };
    if forecast_base(kst) >= early {
        early
    } else {
        forecast_base(kst)
    }
}

/// 키에 `%` 가 있으면 이미 인코딩된 것으로 보고 그대로 쓴다.
fn encode_key(key: &str) -> String {
    if key.contains('%') {
        return key.to_owned();
    }
    let mut out = String::with_capacity(key.len() * 3);
    for byte in key.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn url(operation: &str, key: &Secret, base: &Kst, rows: u32, (x, y): (i32, i32)) -> Secret {
    Secret::new(format!(
        "{BASE}/{operation}?serviceKey={}&pageNo=1&numOfRows={rows}&dataType=JSON\
         &base_date={}&base_time={:02}00&nx={x}&ny={y}",
        encode_key(key.expose()),
        base.date(),
        base.hour,
    ))
}

fn result_code(body: &[u8]) -> Option<String> {
    let json: Value = serde_json::from_slice(body).ok()?;
    json["response"]["header"]["resultCode"]
        .as_str()
        .map(str::to_owned)
}

/// 게이트웨이 오류 코드. JSON 으로도 XML 로도 온다.
fn gateway_code(body: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(body);
    let at = text.find("returnReasonCode")? + "returnReasonCode".len();
    let digits: String = text[at..]
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(char::is_ascii_digit)
        .collect();
    (!digits.is_empty()).then_some(digits)
}

fn items(body: &[u8]) -> Result<Vec<Value>, PortError> {
    let json: Value = serde_json::from_slice(body).map_err(|_| failed())?;
    if json["response"]["header"]["resultCode"] != "00" {
        return Err(failed());
    }
    json["response"]["body"]["items"]["item"]
        .as_array()
        .filter(|items| !items.is_empty())
        .cloned()
        .ok_or_else(failed)
}

/// 값은 "12.3" 같은 글자로 온다. 결측이나 숫자가 아니면 없음.
fn number(value: &Value) -> Option<f64> {
    let number = match value {
        Value::String(text) => text.trim().parse::<f64>().ok()?,
        other => other.as_f64()?,
    };
    (number.is_finite() && number.abs() < MISSING).then_some(number)
}

fn slots(items: &[Value]) -> Vec<(String, Option<i64>, Option<i64>)> {
    let mut slots: Vec<(String, Option<i64>, Option<i64>)> = Vec::new();
    for item in items {
        let (Some(date), Some(time)) = (item["fcstDate"].as_str(), item["fcstTime"].as_str())
        else {
            continue;
        };
        let at = format!("{date}{time}");
        let index = match slots.iter().position(|slot| slot.0 == at) {
            Some(index) => index,
            None => {
                slots.push((at, None, None));
                slots.len() - 1
            }
        };
        let value = number(&item["fcstValue"]).map(|v| v as i64);
        match item["category"].as_str() {
            Some("SKY") => slots[index].1 = value,
            Some("PTY") => slots[index].2 = value,
            _ => {}
        }
    }
    slots.sort_by(|a, b| a.0.cmp(&b.0));
    slots
}

/// 단기예보 SKY: 1 맑음, 3 구름많음, 4 흐림
fn sky_code(code: i64) -> Sky {
    match code {
        1 => Sky::Clear,
        3 => Sky::PartlyCloudy,
        4 => Sky::Cloudy,
        _ => Sky::Unknown,
    }
}

/// 초단기실황 PTY: 1 비, 2 비/눈, 3 눈, 5 빗방울, 6 빗방울눈날림, 7 눈날림
fn nowcast_rain(code: i64) -> Sky {
    match code {
        1 | 2 => Sky::Rain,
        3 | 7 => Sky::Snow,
        5 | 6 => Sky::Drizzle,
        _ => Sky::Unknown,
    }
}

/// 단기예보 PTY: 1 비, 2 비/눈, 3 눈, 4 소나기
fn forecast_rain(code: i64) -> Sky {
    match code {
        1 | 2 => Sky::Rain,
        3 => Sky::Snow,
        4 => Sky::Shower,
        _ => Sky::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Response;
    use crate::time::kst;
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn grid_of_known_places() {
        assert_eq!(grid(37.5665, 126.9780), (60, 127));
        assert_eq!(grid(37.6, 127.0), (60, 127));
        assert!(in_korea(grid(35.2, 129.1)));
        assert!(!in_korea(grid(51.5, -0.1)));
        assert!(!in_korea(grid(-33.9, 151.2)));
    }

    #[test]
    fn base_times() {
        let hm = |k: Kst| (k.date(), k.hour, k.minute);
        assert_eq!(
            hm(nowcast_base(&kst(2026, 11, 27, 0, 39))),
            ("20261126".into(), 23, 0)
        );
        assert_eq!(
            hm(nowcast_base(&kst(2026, 11, 27, 0, 41))),
            ("20261127".into(), 0, 0)
        );
        assert_eq!(
            hm(forecast_base(&kst(2026, 11, 27, 2, 9))),
            ("20261126".into(), 23, 0)
        );
        assert_eq!(
            hm(forecast_base(&kst(2026, 11, 27, 2, 11))),
            ("20261127".into(), 2, 0)
        );
        assert_eq!(
            hm(forecast_base(&kst(2026, 11, 27, 23, 59))),
            ("20261127".into(), 23, 0)
        );
        assert_eq!(
            hm(forecast_base(&kst(2026, 11, 27, 13, 0))),
            ("20261127".into(), 11, 0)
        );
        assert_eq!(
            hm(daily_base(&kst(2026, 11, 27, 2, 9))),
            ("20261126".into(), 23, 0)
        );
        assert_eq!(
            hm(daily_base(&kst(2026, 11, 27, 2, 11))),
            ("20261127".into(), 2, 0)
        );
        assert_eq!(
            hm(daily_base(&kst(2026, 11, 27, 18, 0))),
            ("20261127".into(), 2, 0)
        );
    }

    #[test]
    fn keys_are_encoded_once() {
        assert_eq!(encode_key("ab+c/d=="), "ab%2Bc%2Fd%3D%3D");
        assert_eq!(encode_key("ab%2Bc"), "ab%2Bc");
    }

    fn nowcast_body(t1h: &str, pty: &str) -> String {
        format!(
            r#"{{"response":{{"header":{{"resultCode":"00","resultMsg":"NORMAL_SERVICE"}},"body":{{"dataType":"JSON","items":{{"item":[{{"baseDate":"20261127","baseTime":"0000","category":"PTY","nx":60,"ny":127,"obsrValue":"{pty}"}},{{"baseDate":"20261127","baseTime":"0000","category":"T1H","nx":60,"ny":127,"obsrValue":"{t1h}"}}]}},"pageNo":1,"numOfRows":10,"totalCount":2}}}}}}"#
        )
    }

    fn forecast_item(date: &str, time: &str, category: &str, value: &str) -> String {
        format!(
            r#"{{"baseDate":"20261127","baseTime":"0200","category":"{category}","fcstDate":"{date}","fcstTime":"{time}","fcstValue":"{value}","nx":60,"ny":127}}"#
        )
    }

    fn forecast_body(items: &[String]) -> String {
        format!(
            r#"{{"response":{{"header":{{"resultCode":"00","resultMsg":"NORMAL_SERVICE"}},"body":{{"dataType":"JSON","items":{{"item":[{}]}},"pageNo":1,"numOfRows":100,"totalCount":{}}}}}}}"#,
            items.join(","),
            items.len()
        )
    }

    struct Fake {
        nowcast: String,
        forecast: String,
        daily: String,
        calls: Mutex<Vec<String>>,
    }

    impl Get for Fake {
        fn get(&self, url: &Secret) -> Result<Response, PortError> {
            let url = url.expose();
            self.calls.lock().unwrap().push(url.to_owned());
            let body = if url.contains(NOWCAST) {
                &self.nowcast
            } else if url.contains("numOfRows=400") {
                &self.daily
            } else {
                &self.forecast
            };
            Ok(Response {
                status: 200,
                body: body.clone().into_bytes(),
            })
        }
    }

    /// 2026-11-27 14:30 KST
    fn afternoon() -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_795_707_000 + 14 * 3600)
    }

    fn seoul() -> Place {
        Place {
            latitude: 376,
            longitude: 1270,
        }
    }

    fn fake(nowcast: String, sky: &str, pty: &str, low: &str, high: &str) -> Arc<Fake> {
        Arc::new(Fake {
            nowcast,
            forecast: forecast_body(&[
                forecast_item("20261127", "1300", "SKY", "4"),
                forecast_item("20261127", "1500", "SKY", sky),
                forecast_item("20261127", "1500", "PTY", pty),
                forecast_item("20261127", "1500", "TMP", "12"),
            ]),
            daily: forecast_body(&[
                forecast_item("20261127", "0600", "TMN", low),
                forecast_item("20261127", "1500", "TMX", high),
                forecast_item("20261128", "0600", "TMN", "-20.0"),
            ]),
            calls: Mutex::default(),
        })
    }

    #[test]
    fn reads_nowcast_forecast_and_daily() {
        let get = fake(nowcast_body("10.2", "0"), "3", "0", "5.0", "16.0");
        let kma = Kma::new(get.clone());
        let key = Secret::new("ab+c");
        let weather = kma.fetch(seoul(), &key, afternoon()).unwrap();
        assert_eq!(weather.temperature, 10.2);
        assert_eq!((weather.low, weather.high), (5.0, 16.0));
        assert_eq!(weather.sky, Sky::PartlyCloudy as i32);
        assert!(weather.is_day);
        assert_eq!(weather.provider, Provider::Kma as i32);
        let calls = get.calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 3);
        assert!(calls[0].contains("base_date=20261127&base_time=1300&nx=60&ny=127"));
        assert!(calls[1].contains("base_time=1400"));
        assert!(calls[2].contains("base_time=0200&"));
        assert!(calls.iter().all(|url| url.contains("serviceKey=ab%2Bc&")));
        // 같은 발표, 같은 날이면 실황만 다시 부른다.
        kma.fetch(seoul(), &key, afternoon() + Duration::from_secs(900))
            .unwrap();
        assert_eq!(get.calls.lock().unwrap().len(), 4);
    }

    #[test]
    fn rain_comes_from_the_nowcast_first() {
        let key = Secret::new("k");
        let weather = |nowcast_pty: &str, forecast_pty: &str| {
            let get = fake(
                nowcast_body("3.0", nowcast_pty),
                "1",
                forecast_pty,
                "1.0",
                "5.0",
            );
            Sky::try_from(Kma::new(get).fetch(seoul(), &key, afternoon()).unwrap().sky).unwrap()
        };
        assert_eq!(weather("0", "0"), Sky::Clear);
        assert_eq!(weather("3", "0"), Sky::Snow);
        assert_eq!(weather("5", "4"), Sky::Drizzle);
        assert_eq!(weather("0", "4"), Sky::Shower);
    }

    #[test]
    fn bad_values_fail() {
        let key = Secret::new("k");
        let fails = |get: Arc<Fake>| Kma::new(get).fetch(seoul(), &key, afternoon()).is_err();
        assert!(fails(fake(
            nowcast_body("-999", "0"),
            "1",
            "0",
            "1.0",
            "5.0"
        )));
        assert!(fails(fake(nowcast_body("눈", "0"), "1", "0", "1.0", "5.0")));
        assert!(fails(fake(
            nowcast_body("3.0", "0"),
            "1",
            "0",
            "9.0",
            "5.0"
        )));
        assert!(fails(fake(
            nowcast_body("3.0", "0"),
            "2",
            "0",
            "1.0",
            "5.0"
        )));
        let mut no_high = fake(nowcast_body("3.0", "0"), "1", "0", "1.0", "5.0");
        Arc::get_mut(&mut no_high).unwrap().daily =
            forecast_body(&[forecast_item("20261127", "0600", "TMN", "1.0")]);
        assert!(fails(no_high));
        let mut early = fake(nowcast_body("3.0", "0"), "1", "0", "1.0", "5.0");
        Arc::get_mut(&mut early).unwrap().nowcast =
            r#"{"response":{"header":{"resultCode":"10","resultMsg":"현재 이후의 base_time은 호출할 수 없습니다."}}}"#.to_owned();
        assert!(fails(early));
        // 국외는 부르지도 않는다.
        let get = fake(nowcast_body("3.0", "0"), "1", "0", "1.0", "5.0");
        let london = Place {
            latitude: 515,
            longitude: -1,
        };
        assert!(
            Kma::new(get.clone())
                .fetch(london, &key, afternoon())
                .is_err()
        );
        assert!(get.calls.lock().unwrap().is_empty());
    }

    struct Answer(Result<(u16, &'static str), ()>);

    impl Get for Answer {
        fn get(&self, _url: &Secret) -> Result<Response, PortError> {
            self.0
                .map(|(status, body)| Response {
                    status,
                    body: body.as_bytes().to_vec(),
                })
                .map_err(|_| PortError::new("연결 안 됨"))
        }
    }

    #[test]
    fn key_checks() {
        let check =
            |answer| Kma::new(Arc::new(Answer(answer))).check(&Secret::new("k"), afternoon());
        let ok =
            r#"{"response":{"header":{"resultCode":"00","resultMsg":"NORMAL_SERVICE"},"body":{}}}"#;
        let wrong_json = r#"{
  "OpenAPI_ServiceResponse": {
    "cmmMsgHeader": {
      "errMsg": "SERVICE_KEY_IS_NOT_REGISTERED_ERROR",
      "returnAuthMsg": "등록되지 않은 서비스키",
      "returnReasonCode": "30"
    }
  }
}"#;
        let limit_xml = "<OpenAPI_ServiceResponse><cmmMsgHeader><errMsg>SERVICE ERROR</errMsg><returnAuthMsg>LIMITED_NUMBER_OF_SERVICE_REQUESTS_EXCEEDS_ERROR</returnAuthMsg><returnReasonCode>22</returnReasonCode></cmmMsgHeader></OpenAPI_ServiceResponse>";
        assert_eq!(check(Ok((200, ok))), KeyCheck::Good);
        assert_eq!(check(Ok((403, wrong_json))), KeyCheck::Wrong);
        assert_eq!(check(Ok((200, limit_xml))), KeyCheck::Limit);
        assert_eq!(check(Ok((429, ""))), KeyCheck::Limit);
        assert_eq!(check(Err(())), KeyCheck::Offline);
        assert_eq!(check(Ok((500, "oops"))), KeyCheck::Unknown);
    }

    #[test]
    fn urls_and_errors_hide_the_key() {
        let key = Secret::new("SECRETKEY");
        let url = url(NOWCAST, &key, &kst(2026, 11, 27, 0, 0), 10, (60, 127));
        assert!(!format!("{url:?}").contains("SECRETKEY"));
        let get = Arc::new(Answer(Ok((500, "SECRETKEY"))));
        let error = Kma::new(get).fetch(seoul(), &key, afternoon()).unwrap_err();
        assert!(!error.0.contains("SECRETKEY") && !error.0.contains("37.6"));
    }
}
