use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpStream};
use std::time::Duration;

use sidedesk_domain::{Phone, PhoneLinks, PortError};

const SERVER: SocketAddr = SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::LOCALHOST), 5037);
const SOCKET: &str = "localabstract:sidedesk-dev";
const CONNECT: Duration = Duration::from_secs(1);
const TRANSFER: Duration = Duration::from_secs(2);

/// 이미 켜져 있는 adb 서버와 말한다. 서버를 켜거나 끄지 않고 포트를 새로 열지 않는다.
pub struct AdbLinks {
    server: SocketAddr,
}

impl Default for AdbLinks {
    fn default() -> Self {
        Self { server: SERVER }
    }
}

enum Refusal {
    Said(String),
    Broken(PortError),
}

impl From<io::Error> for Refusal {
    fn from(error: io::Error) -> Self {
        Self::Broken(broken(error))
    }
}

impl From<Refusal> for PortError {
    fn from(refusal: Refusal) -> Self {
        match refusal {
            Refusal::Said(reason) => PortError::new(format!("adb 가 거절함: {reason}")),
            Refusal::Broken(error) => error,
        }
    }
}

fn broken(error: io::Error) -> PortError {
    PortError::new(format!("adb 와 통신하지 못함: {error}"))
}

impl AdbLinks {
    fn open(&self) -> Result<TcpStream, PortError> {
        let stream = TcpStream::connect_timeout(&self.server, CONNECT)
            .map_err(|error| PortError::new(format!("adb 서버에 접속하지 못함: {error}")))?;
        stream.set_read_timeout(Some(TRANSFER)).map_err(broken)?;
        stream.set_write_timeout(Some(TRANSFER)).map_err(broken)?;
        Ok(stream)
    }
}

impl PhoneLinks for AdbLinks {
    fn label(&self) -> &str {
        "adb"
    }

    fn attached(&self) -> Result<Vec<Phone>, PortError> {
        let mut stream = self.open()?;
        request(&mut stream, "host:devices")?;
        let list = read_text(&mut stream)?;
        Ok(list
            .lines()
            .filter_map(|line| line.split_once('\t'))
            .map(|(id, state)| Phone {
                id: id.to_owned(),
                blocked: blocked(state),
            })
            .collect())
    }

    fn send(&self, id: &str, bytes: &[u8]) -> Result<(), PortError> {
        let mut stream = self.open()?;
        request(&mut stream, &format!("host:transport:{id}"))?;
        request(&mut stream, SOCKET).map_err(|refusal| match refusal {
            Refusal::Said(_) => PortError::new("폰의 앱이 꺼져 있거나 뒤에 있음"),
            Refusal::Broken(error) => error,
        })?;
        stream.write_all(bytes).map_err(broken)?;
        stream.shutdown(Shutdown::Write).map_err(broken)
    }
}

fn blocked(state: &str) -> Option<String> {
    match state {
        "device" => None,
        "unauthorized" => Some("폰에서 USB 디버깅을 허용하지 않음".to_owned()),
        "offline" => Some("폰이 응답하지 않음".to_owned()),
        other => Some(format!("보낼 수 없는 상태: {other}")),
    }
}

fn request(stream: &mut TcpStream, command: &str) -> Result<(), Refusal> {
    stream.write_all(format!("{:04x}{command}", command.len()).as_bytes())?;
    let mut status = [0; 4];
    stream.read_exact(&mut status)?;
    match &status {
        b"OKAY" => Ok(()),
        b"FAIL" => Err(Refusal::Said(read_text(stream)?)),
        _ => Err(Refusal::Broken(PortError::new("adb 응답이 이상함"))),
    }
}

fn read_text(stream: &mut TcpStream) -> Result<String, Refusal> {
    let strange = || Refusal::Broken(PortError::new("adb 응답이 이상함"));
    let mut header = [0; 4];
    stream.read_exact(&mut header)?;
    if !header.iter().all(u8::is_ascii_hexdigit) {
        return Err(strange());
    }
    let header = std::str::from_utf8(&header).map_err(|_| strange())?;
    let length = usize::from_str_radix(header, 16).map_err(|_| strange())?;
    let mut body = vec![0; length];
    stream.read_exact(&mut body)?;
    Ok(String::from_utf8_lossy(&body).into_owned())
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;
    use std::thread::{self, JoinHandle};

    use super::*;

    type Served = (Vec<String>, Vec<u8>);

    /// 요청을 하나 받을 때마다 답을 하나 보내고, 다 쓰면 남은 바이트를 돌려준다.
    fn adb(answers: &'static [&'static str]) -> (AdbLinks, JoinHandle<Served>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let server = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut asked = Vec::new();
            for answer in answers {
                let Ok(command) = read_text(&mut stream) else {
                    panic!("요청을 읽지 못함")
                };
                asked.push(command);
                stream.write_all(answer.as_bytes()).unwrap();
            }
            let mut rest = Vec::new();
            if answers.len() > 1 {
                stream.read_to_end(&mut rest).unwrap();
            }
            (asked, rest)
        });
        (AdbLinks { server }, handle)
    }

    #[test]
    fn lists_phones_with_the_reason_they_are_blocked() {
        let (links, server) = adb(&["OKAY0021a\tdevice\nb\tunauthorized\nc\toffline"]);
        let phones = links.attached().unwrap();
        assert_eq!(phones[0].blocked, None);
        assert!(phones[1].blocked.as_ref().unwrap().contains("허용"));
        assert_eq!(phones[2].id, "c");
        assert!(phones[2].blocked.is_some());
        assert_eq!(server.join().unwrap().0, ["host:devices"]);
    }

    #[test]
    fn an_empty_list_is_fine() {
        let (links, _server) = adb(&["OKAY0000"]);
        assert_eq!(links.attached(), Ok(Vec::new()));
    }

    #[test]
    fn sends_the_bytes_to_the_app() {
        let (links, server) = adb(&["OKAY", "OKAY"]);
        assert_eq!(links.send("a", b"hi"), Ok(()));
        let (asked, rest) = server.join().unwrap();
        assert_eq!(asked, ["host:transport:a", "localabstract:sidedesk-dev"]);
        assert_eq!(rest, b"hi");
    }

    #[test]
    fn tells_a_missing_app_from_a_missing_phone() {
        let (links, _server) = adb(&["OKAY", "FAIL0006closed"]);
        assert!(links.send("a", b"hi").unwrap_err().0.contains("앱이 꺼져"));
        let (links, _server) = adb(&["FAIL0004gone"]);
        assert!(links.send("a", b"hi").unwrap_err().0.contains("gone"));
    }

    #[test]
    fn reports_strange_answers() {
        for answers in [
            &["WHAT"] as &[&str],
            &["OKAY+123"],
            &["OKAYzzzz"],
            &["OKAY0010short"],
            &["OK"],
        ] {
            let (links, _server) = adb(answers);
            assert!(links.attached().is_err(), "{answers:?}");
        }
    }

    #[test]
    fn reports_a_server_that_is_off() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let server = listener.local_addr().unwrap();
        drop(listener);
        let error = AdbLinks { server }.attached().unwrap_err();
        assert!(error.0.contains("adb 서버"));
    }
}
