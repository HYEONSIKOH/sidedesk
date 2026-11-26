//! 맥 HTTP GET. NSURLSession 을 디스크 캐시 없이(ephemeral) 하나만 만들어 쓴다.

use std::sync::mpsc;
use std::time::Duration;

use block2::RcBlock;
use objc2::rc::{Retained, autoreleasepool};
use objc2_foundation::{
    NSData, NSError, NSHTTPURLResponse, NSString, NSURL, NSURLResponse, NSURLSession,
    NSURLSessionConfiguration,
};
use sidedesk_domain::{PortError, Secret};
use sidedesk_weather::{Get, Response};

/// 받는 사이 쉬는 시간과 전체 시간 모두 이 안이다.
const LIMIT: f64 = 10.0;
/// 기다리는 쪽 상한. 넘으면 취소한다.
const WAIT: Duration = Duration::from_secs(12);
/// 응답 상한(기상청 하루치 400줄을 덮는다)
const MAX_BODY: usize = 256 * 1024;

struct Session(Retained<NSURLSession>);
// SAFETY: NSURLSession 은 여러 스레드에서 써도 된다(애플 문서). 작업 만들기와 취소만 한다.
unsafe impl Send for Session {}
// SAFETY: 위와 같다.
unsafe impl Sync for Session {}

pub struct MacGet {
    session: Session,
}

impl Default for MacGet {
    fn default() -> Self {
        let configuration = NSURLSessionConfiguration::ephemeralSessionConfiguration();
        configuration.setTimeoutIntervalForRequest(LIMIT);
        configuration.setTimeoutIntervalForResource(LIMIT);
        Self {
            session: Session(NSURLSession::sessionWithConfiguration(&configuration)),
        }
    }
}

type Outcome = Result<Response, ()>;

impl Get for MacGet {
    fn get(&self, url: &Secret) -> Result<Response, PortError> {
        // 오류 문장에 주소(키, 좌표)나 시스템 오류 설명을 넣지 않는다.
        let failed = || PortError::new("인터넷에서 받지 못했습니다");
        autoreleasepool(|_| {
            let address = NSString::from_str(url.expose());
            let url = NSURL::URLWithString(&address).ok_or_else(failed)?;
            let (sender, receiver) = mpsc::channel::<Outcome>();
            // 블록 안에서는 panic 하지 않는다. 늦게 와서 보내기에 실패해도 무시한다.
            let block = RcBlock::new(
                move |data: *mut NSData, response: *mut NSURLResponse, error: *mut NSError| {
                    // SAFETY: NSURLSession 이 넘긴 포인터는 블록이 도는 동안 살아 있다.
                    let outcome = unsafe { outcome(data, response, error) };
                    let _ = sender.send(outcome);
                },
            );
            // SAFETY: url 과 block 은 살아 있고, 블록 모양은 이 메서드가 바라는 것과 같다.
            let task = unsafe {
                self.session
                    .0
                    .dataTaskWithURL_completionHandler(&url, &block)
            };
            task.resume();
            match receiver.recv_timeout(WAIT) {
                Ok(Ok(response)) => Ok(response),
                Ok(Err(())) => Err(failed()),
                Err(_) => {
                    task.cancel();
                    Err(failed())
                }
            }
        })
    }
}

/// # Safety
/// 세 포인터는 null 이거나 살아 있는 객체여야 한다.
unsafe fn outcome(data: *mut NSData, response: *mut NSURLResponse, error: *mut NSError) -> Outcome {
    if !error.is_null() || response.is_null() {
        return Err(());
    }
    // SAFETY: 위에서 null 이 아님을 봤다.
    let response = unsafe { &*response };
    let status = response
        .downcast_ref::<NSHTTPURLResponse>()
        .map(|http| http.statusCode())
        .ok_or(())?;
    let body = if data.is_null() {
        Vec::new()
    } else {
        // SAFETY: null 이 아님을 봤다.
        let data = unsafe { &*data };
        if data.length() > MAX_BODY {
            return Err(());
        }
        data.to_vec()
    };
    Ok(Response {
        status: u16::try_from(status).map_err(|_| ())?,
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    #[ignore = "인터넷에 나간다. 손으로 돌린다"]
    fn open_meteo_seoul() {
        let response = MacGet::default()
            .get(&Secret::new(
                "https://api.open-meteo.com/v1/forecast?latitude=37.6&longitude=127.0\
                 &current=temperature_2m,weather_code,is_day\
                 &daily=temperature_2m_max,temperature_2m_min&timezone=auto&forecast_days=1",
            ))
            .unwrap();
        assert_eq!(response.status, 200);
        eprintln!("{}", String::from_utf8_lossy(&response.body));
    }

    #[test]
    #[ignore = "인터넷에 나간다. 손으로 돌린다"]
    fn a_missing_page_is_a_status_not_an_error() {
        let response = MacGet::default()
            .get(&Secret::new("https://api.open-meteo.com/v1/nothing-here"))
            .unwrap();
        assert_eq!(response.status, 404);
    }

    #[test]
    #[ignore = "닿지 않는 주소로 시간 제한을 본다. 손으로 돌린다"]
    fn an_unreachable_host_gives_up_in_time() {
        let start = Instant::now();
        let Err(error) = MacGet::default().get(&Secret::new("https://10.255.255.1/")) else {
            panic!("닿지 않는 주소인데 응답이 옴");
        };
        assert!(start.elapsed() < WAIT + Duration::from_secs(1));
        assert!(!error.0.contains("10.255"));
    }
}
