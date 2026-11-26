use std::io::{self, ErrorKind, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::time::Duration;

use sidedesk_domain::{Conn, Link, Phone, PhoneLinks, PortError, Received, Source};

const SERVER: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 5037);
const SOCKET: &str = "localabstract:sidedesk";
const CONNECT: Duration = Duration::from_secs(1);
const TRANSFER: Duration = Duration::from_secs(2);
/// 연결 위에서 한 번 읽을 때 기다리는 시간. 그 사이에 보낼 것과 시간을 따진다.
const POLL: Duration = Duration::from_millis(250);

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
    fn open(&self, read: Option<Duration>) -> Result<TcpStream, PortError> {
        let stream = TcpStream::connect_timeout(&self.server, CONNECT)
            .map_err(|error| PortError::new(format!("adb 서버에 접속하지 못함: {error}")))?;
        stream.set_read_timeout(read).map_err(broken)?;
        stream.set_write_timeout(Some(TRANSFER)).map_err(broken)?;
        Ok(stream)
    }
}

impl PhoneLinks for AdbLinks {
    fn link(&self) -> Link {
        Link::Usb
    }

    fn source(&self) -> Source {
        Source::new(Link::Usb, "adb")
    }

    fn attached(&self) -> Result<Vec<Phone>, PortError> {
        let mut stream = self.open(Some(TRANSFER))?;
        request(&mut stream, "host:devices-l")?;
        Ok(phones(&read_text(&mut stream)?))
    }

    fn watch(&self, changed: &mut dyn FnMut(Vec<Phone>)) -> Result<(), PortError> {
        // 바뀔 때만 오므로 시간 제한을 두지 않는다.
        let mut stream = self.open(None)?;
        request(&mut stream, "host:track-devices-l")?;
        loop {
            changed(phones(&read_text(&mut stream)?));
        }
    }

    fn connect(&self, id: &str) -> Result<Box<dyn Conn>, PortError> {
        let mut stream = self.open(Some(TRANSFER))?;
        request(&mut stream, &format!("host:transport:{id}"))?;
        request(&mut stream, SOCKET).map_err(|refusal| match refusal {
            Refusal::Said(_) => PortError::new("기기에서 SideDesk 앱을 열어 주세요"),
            Refusal::Broken(error) => error,
        })?;
        stream.set_read_timeout(Some(POLL)).map_err(broken)?;
        Ok(Box::new(AdbConn(stream)))
    }
}

struct AdbConn(TcpStream);

impl Conn for AdbConn {
    fn receive(&mut self, buffer: &mut [u8]) -> Result<Received, PortError> {
        match self.0.read(buffer) {
            Ok(0) => Ok(Received::Closed),
            Ok(count) => Ok(Received::Bytes(count)),
            Err(error)
                if matches!(
                    error.kind(),
                    ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
                ) =>
            {
                Ok(Received::Nothing)
            }
            Err(error)
                if matches!(
                    error.kind(),
                    ErrorKind::ConnectionReset | ErrorKind::BrokenPipe
                ) =>
            {
                Ok(Received::Closed)
            }
            Err(error) => Err(broken(error)),
        }
    }

    fn send(&mut self, bytes: &[u8]) -> Result<(), PortError> {
        self.0.write_all(bytes).map_err(broken)
    }
}

/// `devices-l` 한 줄: 일련번호, 상태, 그 뒤로 `product:… model:… device:…` 가 공백으로 이어진다.
fn phones(list: &str) -> Vec<Phone> {
    list.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let id = words.next()?;
            let state = words.next()?;
            let model = words
                .find_map(|word| word.strip_prefix("model:"))
                .map(|model| model.replace('_', " "));
            Some(Phone::usb(id, model, blocked(state)))
        })
        .collect()
}

fn blocked(state: &str) -> Option<String> {
    match state {
        "device" => None,
        "unauthorized" => Some("기기에서 USB 디버깅을 허용해 주세요".to_owned()),
        "offline" => Some("기기가 응답하지 않습니다".to_owned()),
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

    /// 요청을 하나 받을 때마다 답을 하나 보낸다. 다 보낸 뒤 `rest` 를 보내고, 받은 나머지를 돌려준다.
    fn adb(answers: &'static [&'static str], rest: &'static str) -> (AdbLinks, JoinHandle<Served>) {
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
            stream.write_all(rest.as_bytes()).unwrap();
            let mut got = Vec::new();
            stream
                .set_read_timeout(Some(Duration::from_millis(300)))
                .unwrap();
            let _ = stream.read_to_end(&mut got);
            (asked, got)
        });
        (AdbLinks { server }, handle)
    }

    #[test]
    fn lists_phones_with_the_reason_they_are_blocked() {
        let list = "SERIAL00001            device usb:1X product:a71xskt model:SM_A716S device:a71x transport_id:23\n\
                    b                      unauthorized usb:2X transport_id:24\n\
                    c                      offline transport_id:25\n";
        let answer: &'static str =
            Box::leak(format!("OKAY{:04x}{list}", list.len()).into_boxed_str());
        let answers: &'static [&'static str] = Box::leak(vec![answer].into_boxed_slice());
        let (links, server) = adb(answers, "");
        let phones = links.attached().unwrap();
        assert_eq!(phones[0].id, "SERIAL00001");
        assert_eq!(phones[0].model.as_deref(), Some("SM A716S"));
        assert_eq!(phones[0].blocked, None);
        assert_eq!(phones[1].model, None);
        assert!(phones[1].blocked.as_ref().unwrap().contains("허용"));
        assert!(phones[2].blocked.is_some());
        assert_eq!(server.join().unwrap().0, ["host:devices-l"]);
    }

    #[test]
    fn watches_phones_come_and_go() {
        let (links, server) = adb(&["OKAY"], "0009a device\n00000009a device\n");
        let mut seen = Vec::new();
        let ended = links.watch(&mut |phones| seen.push(phones.len()));
        assert!(ended.is_err());
        assert_eq!(seen, [1, 0, 1]);
        assert_eq!(server.join().unwrap().0, ["host:track-devices-l"]);
    }

    #[test]
    fn talks_to_the_app_through_the_connection() {
        let (links, server) = adb(&["OKAY", "OKAY"], "pong");
        let mut conn = links.connect("a").unwrap();
        conn.send(b"ping").unwrap();
        let mut buffer = [0; 16];
        let mut got = Vec::new();
        loop {
            match conn.receive(&mut buffer).unwrap() {
                Received::Bytes(count) => got.extend_from_slice(&buffer[..count]),
                Received::Nothing => {}
                Received::Closed => break,
            }
        }
        assert_eq!(got, b"pong");
        let (asked, sent) = server.join().unwrap();
        assert_eq!(asked, ["host:transport:a", "localabstract:sidedesk"]);
        assert_eq!(sent, b"ping");
    }

    #[test]
    fn tells_a_missing_app_from_a_missing_phone() {
        let (links, _server) = adb(&["OKAY", "FAIL0006closed"], "");
        assert!(links.connect("a").err().unwrap().0.contains("앱을 열어"));
        let (links, _server) = adb(&["FAIL0004gone"], "");
        assert!(links.connect("a").err().unwrap().0.contains("gone"));
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
            let (links, _server) = adb(answers, "");
            assert!(links.attached().is_err(), "{answers:?}");
        }
    }

    #[test]
    fn reports_a_server_that_is_off() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let server = listener.local_addr().unwrap();
        drop(listener);
        assert!(
            AdbLinks { server }
                .attached()
                .unwrap_err()
                .0
                .contains("adb 서버")
        );
    }
}
