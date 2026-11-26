//! 윈도우 HTTP GET. 운영체제의 WinHTTP(TLS 는 SChannel)를 동기로 쓴다. 캐시가 없다.

use std::ffi::c_void;
use std::ptr;
use std::time::{Duration, Instant};

use sidedesk_domain::{PortError, Secret};
use sidedesk_weather::{Get, Response};
use windows_sys::Win32::Networking::WinHttp::{
    URL_COMPONENTS, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE,
    WINHTTP_INTERNET_SCHEME_HTTPS, WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
    WinHttpCloseHandle, WinHttpConnect, WinHttpCrackUrl, WinHttpOpen, WinHttpOpenRequest,
    WinHttpQueryDataAvailable, WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse,
    WinHttpSendRequest, WinHttpSetTimeouts,
};

use crate::handle::wide;

/// 단계 하나(이름 풀기, 연결, 보내기, 받기)의 상한. 맥 LIMIT 과 같다.
const LIMIT: Duration = Duration::from_secs(10);
/// 전체 마감. 맥 WAIT 과 같다.
const WAIT: Duration = Duration::from_secs(12);
/// 응답 상한(기상청 하루치 400줄을 덮는다)
const MAX_BODY: usize = 256 * 1024;

/// WinHTTP 핸들. 놓을 때 닫는다.
struct Internet(*mut c_void);

impl Internet {
    fn new(handle: *mut c_void) -> Option<Self> {
        (!handle.is_null()).then_some(Self(handle))
    }
}

impl Drop for Internet {
    fn drop(&mut self) {
        // SAFETY: new 에서 받은 유효한 핸들을 한 번만 닫는다.
        unsafe { WinHttpCloseHandle(self.0) };
    }
}

// SAFETY: WinHTTP 세션 핸들은 여러 스레드에서 요청을 만들어도 된다(MS 문서). 세션은 만든 뒤 바꾸지 않고,
// 연결과 요청 핸들은 한 호출 안에서만 쓰고 닫는다.
unsafe impl Send for Internet {}
// SAFETY: 위와 같다.
unsafe impl Sync for Internet {}

/// 다 쓰면 0 으로 채우는 UTF-16. 주소에 키가 들어 있다. WinHTTP 내부 사본은 지우지 못한다.
struct Wiped(Vec<u16>);

impl Drop for Wiped {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

pub struct WinGet {
    /// 세션을 못 열었으면 없고, 받을 때마다 실패한다.
    session: Option<Internet>,
}

impl Default for WinGet {
    fn default() -> Self {
        let agent = wide("SideDesk");
        // SAFETY: 널 끝 이름, 프록시는 시스템 설정을 따른다.
        let session = Internet::new(unsafe {
            WinHttpOpen(
                agent.as_ptr(),
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                ptr::null(),
                ptr::null(),
                0,
            )
        });
        Self { session }
    }
}

/// 마감까지 남은 시간과 단계 상한 가운데 작은 것(밀리초). 마감이 지났으면 없음.
fn step(deadline: Instant) -> Option<i32> {
    let left = deadline.checked_duration_since(Instant::now())?;
    let millis = left.min(LIMIT).as_millis() as i32;
    (millis > 0).then_some(millis)
}

impl Get for WinGet {
    fn get(&self, url: &Secret) -> Result<Response, PortError> {
        // 오류 문장에 주소(키, 좌표)나 시스템 오류 번호를 넣지 않는다.
        let failed = || {
            // 시험에서만 원인 번호를 남긴다(오류 문장에는 넣지 않는다).
            #[cfg(test)]
            tests::remember_error();
            PortError::new("인터넷에서 받지 못했습니다")
        };
        let deadline = Instant::now() + WAIT;
        let session = self.session.as_ref().ok_or_else(failed)?;
        let address = Wiped(wide(url.expose()));
        let mut parts = URL_COMPONENTS {
            dwStructSize: size_of::<URL_COMPONENTS>() as u32,
            // u32::MAX 는 "원문 안을 가리켜 달라"는 뜻
            dwSchemeLength: u32::MAX,
            dwHostNameLength: u32::MAX,
            dwUrlPathLength: u32::MAX,
            dwExtraInfoLength: u32::MAX,
            ..Default::default()
        };
        // SAFETY: 널 끝 주소와 크기를 채운 구조체. 돌려받는 포인터는 address 안을 가리킨다.
        if unsafe { WinHttpCrackUrl(address.0.as_ptr(), 0, 0, &mut parts) } == 0 {
            return Err(failed());
        }
        // SAFETY: CrackUrl 이 준 포인터와 길이는 address 안이다(address 는 이 함수 끝까지 산다).
        let piece = |start: *const u16, length: u32| -> Wiped {
            let mut part = if start.is_null() {
                Vec::new()
            } else {
                unsafe { std::slice::from_raw_parts(start, length as usize) }.to_vec()
            };
            part.push(0);
            Wiped(part)
        };
        let host = piece(parts.lpszHostName, parts.dwHostNameLength);
        let mut object = piece(parts.lpszUrlPath, parts.dwUrlPathLength);
        object.0.pop();
        let extra = piece(parts.lpszExtraInfo, parts.dwExtraInfoLength);
        object.0.extend_from_slice(&extra.0);
        if object.0.len() == 1 {
            object.0.insert(0, u16::from(b'/'));
        }
        let secure = parts.nScheme == WINHTTP_INTERNET_SCHEME_HTTPS;

        // SAFETY: 살아 있는 세션과 널 끝 이름
        let connect =
            Internet::new(unsafe { WinHttpConnect(session.0, host.0.as_ptr(), parts.nPort, 0) })
                .ok_or_else(failed)?;
        let verb = wide("GET");
        // SAFETY: 살아 있는 연결과 널 끝 글자들. 받을 종류는 기본값.
        let request = Internet::new(unsafe {
            WinHttpOpenRequest(
                connect.0,
                verb.as_ptr(),
                object.0.as_ptr(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                if secure { WINHTTP_FLAG_SECURE } else { 0 },
            )
        })
        .ok_or_else(failed)?;
        drop((object, extra, host));

        // 단계마다 남은 시간으로 시간 제한을 다시 건다.
        let limit = |request: &Internet| -> Result<(), PortError> {
            let millis = step(deadline).ok_or_else(failed)?;
            // SAFETY: 살아 있는 요청 핸들
            let set = unsafe { WinHttpSetTimeouts(request.0, millis, millis, millis, millis) };
            (set != 0).then_some(()).ok_or_else(failed)
        };

        limit(&request)?;
        // SAFETY: 머리와 본문 없이 보낸다.
        if unsafe { WinHttpSendRequest(request.0, ptr::null(), 0, ptr::null(), 0, 0, 0) } == 0 {
            return Err(failed());
        }
        limit(&request)?;
        // SAFETY: 보낸 요청의 응답을 받는다.
        if unsafe { WinHttpReceiveResponse(request.0, ptr::null_mut()) } == 0 {
            return Err(failed());
        }
        let mut status = 0u32;
        let mut length = size_of::<u32>() as u32;
        // SAFETY: 숫자 하나를 받는 버퍼
        let read = unsafe {
            WinHttpQueryHeaders(
                request.0,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                ptr::null(),
                (&raw mut status).cast(),
                &mut length,
                ptr::null_mut(),
            )
        };
        if read == 0 {
            return Err(failed());
        }
        let mut body = Vec::new();
        loop {
            limit(&request)?;
            let mut available = 0u32;
            // SAFETY: 받을 수 있는 크기를 묻는다.
            if unsafe { WinHttpQueryDataAvailable(request.0, &mut available) } == 0 {
                return Err(failed());
            }
            if available == 0 {
                break;
            }
            if body.len() + available as usize > MAX_BODY {
                return Err(failed());
            }
            let start = body.len();
            body.resize(start + available as usize, 0);
            let mut got = 0u32;
            // SAFETY: body 끝에 available 바이트 자리를 만들었다.
            let read = unsafe {
                WinHttpReadData(
                    request.0,
                    body[start..].as_mut_ptr().cast(),
                    available,
                    &mut got,
                )
            };
            if read == 0 {
                return Err(failed());
            }
            body.truncate(start + got as usize);
        }
        Ok(Response {
            status: u16::try_from(status).map_err(|_| failed())?,
            body,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    thread_local! {
        static LAST: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    }

    pub(super) fn remember_error() {
        // SAFETY: 이 스레드의 마지막 오류 번호를 읽기만 한다.
        let code = unsafe { windows_sys::Win32::Foundation::GetLastError() };
        LAST.with(|last| last.set(code));
    }

    /// 실패하면 WinHTTP 오류 번호를 찍고, 일시적인 망 문제를 생각해 한 번 더 시도한다.
    fn get_twice(url: &str) -> Response {
        let get = WinGet::default();
        match get.get(&Secret::new(url)) {
            Ok(response) => response,
            Err(_) => {
                eprintln!("첫 시도 실패, WinHTTP 오류 {}", LAST.with(|l| l.get()));
                std::thread::sleep(Duration::from_secs(3));
                get.get(&Secret::new(url)).unwrap_or_else(|_| {
                    panic!("두 번 실패, WinHTTP 오류 {}", LAST.with(|l| l.get()))
                })
            }
        }
    }

    #[test]
    #[ignore = "인터넷에 나간다. CI 윈도우 작업에서 돌린다"]
    fn windows_only_open_meteo_seoul() {
        let response = get_twice(
            "https://api.open-meteo.com/v1/forecast?latitude=37.6&longitude=127.0\
             &current=temperature_2m,weather_code,is_day\
             &daily=temperature_2m_max,temperature_2m_min&timezone=auto&forecast_days=1",
        );
        assert_eq!(response.status, 200);
        let text = String::from_utf8_lossy(&response.body);
        assert!(text.contains("temperature_2m"), "{text}");
    }

    #[test]
    #[ignore = "인터넷에 나간다. CI 윈도우 작업에서 돌린다"]
    fn windows_only_a_missing_page_is_a_status_not_an_error() {
        let response = get_twice("https://api.open-meteo.com/v1/nothing-here");
        assert_eq!(response.status, 404);
    }

    #[test]
    #[ignore = "닿지 않는 주소로 시간 제한을 본다. CI 윈도우 작업에서 돌린다"]
    fn windows_only_an_unreachable_host_gives_up_in_time() {
        let start = Instant::now();
        let Err(error) = WinGet::default().get(&Secret::new("https://10.255.255.1/")) else {
            panic!("닿지 않는 주소인데 응답이 옴");
        };
        eprintln!("닿지 않는 주소 {:?}", start.elapsed());
        assert!(start.elapsed() < WAIT + LIMIT);
        assert!(!error.0.contains("10.255"));
    }

    #[test]
    fn a_broken_address_is_an_error_without_the_address() {
        let error = WinGet::default()
            .get(&Secret::new("not a url key=secret"))
            .err()
            .unwrap();
        assert!(!error.0.contains("secret"));
    }

    #[test]
    fn steps_never_outlast_the_deadline() {
        assert_eq!(step(Instant::now() + Duration::from_secs(60)), Some(10_000));
        let short = step(Instant::now() + Duration::from_secs(2)).unwrap();
        assert!(short <= 2_000 && short > 1_000);
        assert_eq!(step(Instant::now()), None);
    }
}
