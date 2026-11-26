//! 아이폰 USB. 운영체제의 usbmuxd(맥은 유닉스 소켓, 윈도우는 애플 장치 서비스)에 접속해
//! 아이폰 앱이 기기 안에서 기다리는 포트로 이어 달라고 한다. 컴퓨터에는 포트를 열지 않는다.
//!
//! 메시지: 머리 16바이트(작은 끝 u32 넷: 전체 길이, 버전 1, 종류 8 = plist, 번호) + XML plist.

use std::collections::HashMap;
use std::io::{self, ErrorKind, Read, Write};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use sidedesk_domain::{Conn, Link, Phone, PhoneLinks, PortError, Received, Source};

const VERSION: u32 = 1;
const PLIST: u32 = 8;
const HEADER: usize = 16;
const MAX_BODY: usize = 1024 * 1024;
const TRANSFER: Duration = Duration::from_secs(2);
/// 연결 위에서 한 번 읽을 때 기다리는 시간(adb 와 같음)
const POLL: Duration = Duration::from_millis(250);
const PREFIX: &str = "ios:";

/// usbmuxd 와 이어진 통로 하나
pub trait MuxStream: Read + Write + Send {
    fn set_read_timeout(&self, time: Option<Duration>) -> io::Result<()>;
}

/// usbmuxd 에 새로 접속한다. 운영체제마다 하나씩 둔다.
pub trait Dial: Send + Sync {
    fn open(&self) -> io::Result<Box<dyn MuxStream>>;
}

#[cfg(unix)]
impl MuxStream for std::os::unix::net::UnixStream {
    fn set_read_timeout(&self, time: Option<Duration>) -> io::Result<()> {
        std::os::unix::net::UnixStream::set_read_timeout(self, time)
    }
}

impl MuxStream for std::net::TcpStream {
    fn set_read_timeout(&self, time: Option<Duration>) -> io::Result<()> {
        std::net::TcpStream::set_read_timeout(self, time)
    }
}

/// 맥의 usbmuxd 유닉스 소켓
#[cfg(unix)]
pub struct UnixDial(pub std::path::PathBuf);

#[cfg(unix)]
impl Default for UnixDial {
    fn default() -> Self {
        Self("/var/run/usbmuxd".into())
    }
}

#[cfg(unix)]
impl Dial for UnixDial {
    fn open(&self) -> io::Result<Box<dyn MuxStream>> {
        let stream = std::os::unix::net::UnixStream::connect(&self.0)?;
        stream.set_write_timeout(Some(TRANSFER))?;
        Ok(Box::new(stream))
    }
}

/// 윈도우 애플 장치 서비스의 usbmuxd. 이미 열려 있는 127.0.0.1:27015 에 접속만 한다.
pub struct TcpDial(pub std::net::SocketAddr);

impl Default for TcpDial {
    fn default() -> Self {
        Self(std::net::SocketAddr::from(([127, 0, 0, 1], 27015)))
    }
}

impl Dial for TcpDial {
    fn open(&self) -> io::Result<Box<dyn MuxStream>> {
        let stream = std::net::TcpStream::connect_timeout(&self.0, Duration::from_secs(1))?;
        stream.set_write_timeout(Some(TRANSFER))?;
        Ok(Box::new(stream))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Device {
    serial: String,
    usb: bool,
}

pub struct UsbmuxLinks {
    dial: Box<dyn Dial>,
    /// 아이폰 앱이 기다리는 포트
    port: u16,
    /// usbmuxd 의 기기 번호마다 일련번호와 USB 여부. 빠짐(`Detached`)은 번호로만 온다.
    devices: Mutex<HashMap<u32, Device>>,
}

impl UsbmuxLinks {
    pub fn new(dial: Box<dyn Dial>, port: u16) -> Self {
        Self {
            dial,
            port,
            devices: Mutex::default(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<u32, Device>> {
        self.devices.lock().unwrap_or_else(|lock| lock.into_inner())
    }

    fn open(&self, read: Option<Duration>) -> Result<Box<dyn MuxStream>, PortError> {
        let stream = self
            .dial
            .open()
            .map_err(|error| PortError::new(format!("usbmuxd 에 접속하지 못함: {error}")))?;
        stream.set_read_timeout(read).map_err(broken)?;
        Ok(stream)
    }

    /// USB 로 붙은 기기만, 일련번호 하나에 한 줄
    fn phones(&self) -> Vec<Phone> {
        let devices = self.lock();
        let mut serials: Vec<&str> = devices
            .values()
            .filter(|device| device.usb)
            .map(|device| device.serial.as_str())
            .collect();
        serials.sort_unstable();
        serials.dedup();
        serials
            .into_iter()
            .map(|serial| Phone::usb(&format!("{PREFIX}{serial}"), Some("iPhone".into()), None))
            .collect()
    }

    fn list(&self) -> Result<HashMap<u32, Device>, PortError> {
        let mut stream = self.open(Some(TRANSFER))?;
        send(stream.as_mut(), 1, &request("ListDevices", &[]))?;
        let answer = receive(stream.as_mut())?;
        let list = answer
            .get("DeviceList")
            .and_then(Value::array)
            .ok_or_else(strange)?;
        Ok(list.iter().filter_map(attached).collect())
    }
}

fn strange() -> PortError {
    PortError::new("usbmuxd 응답이 이상함")
}

fn broken(error: io::Error) -> PortError {
    PortError::new(format!("usbmuxd 와 통신하지 못함: {error}"))
}

/// `Attached` 한 건(목록 줄이든 알림이든)을 기기 번호와 기기로
fn attached(value: &Value) -> Option<(u32, Device)> {
    let properties = value.get("Properties")?;
    let id = value
        .get("DeviceID")
        .or_else(|| properties.get("DeviceID"))?
        .integer()?;
    let serial = properties.get("SerialNumber")?.text()?;
    if serial.is_empty() {
        return None;
    }
    let usb = properties.get("ConnectionType").and_then(Value::text) == Some("USB");
    Some((
        u32::try_from(id).ok()?,
        Device {
            serial: serial.to_owned(),
            usb,
        },
    ))
}

impl PhoneLinks for UsbmuxLinks {
    fn link(&self) -> Link {
        Link::Usb
    }

    fn source(&self) -> Source {
        Source::new(Link::Usb, "usbmux")
    }

    fn attached(&self) -> Result<Vec<Phone>, PortError> {
        let list = self.list()?;
        *self.lock() = list;
        Ok(self.phones())
    }

    fn watch(&self, changed: &mut dyn FnMut(Vec<Phone>)) -> Result<(), PortError> {
        // 지금 목록을 먼저 내고, 그 위에 붙음과 빠짐을 적용한다. 빈 목록을 먼저 내지 않는다
        // (usbmuxd 가 잠깐 끊겼다 붙을 때 살아 있는 아이폰 작업이 끝나지 않게).
        let list = self.list()?;
        *self.lock() = list;
        let mut last = self.phones();
        changed(last.clone());
        // 바뀔 때만 오므로 시간 제한을 두지 않는다(adb `watch` 와 같음). 그래서 `keep_connected` 의
        // stop 은 다음 알림이나 usbmuxd 가 끊길 때 본다. 지금 앱은 stop 을 쓰지 않는다.
        let mut stream = self.open(None)?;
        send(stream.as_mut(), 2, &request("Listen", &[]))?;
        match result(&receive(stream.as_mut())?)? {
            0 => {}
            number => return Err(PortError::new(format!("usbmuxd 가 거절함: {number}"))),
        }
        loop {
            let message = receive(stream.as_mut())?;
            match message.get("MessageType").and_then(Value::text) {
                Some("Attached") => {
                    if let Some((id, device)) = attached(&message) {
                        self.lock().insert(id, device);
                    }
                }
                Some("Detached") => {
                    if let Some(id) = message
                        .get("DeviceID")
                        .and_then(Value::integer)
                        .and_then(|id| u32::try_from(id).ok())
                    {
                        self.lock().remove(&id);
                    }
                }
                // Paired 같은 다른 알림은 건너뛴다.
                _ => continue,
            }
            let now = self.phones();
            if now != last {
                changed(now.clone());
                last = now;
            }
        }
    }

    fn connect(&self, id: &str) -> Result<Box<dyn Conn>, PortError> {
        let serial = id.strip_prefix(PREFIX).unwrap_or(id);
        let device = self
            .lock()
            .iter()
            .find(|(_, device)| device.usb && device.serial == serial)
            .map(|(number, _)| *number)
            .ok_or_else(|| PortError::new("아이폰이 보이지 않습니다"))?;
        let mut stream = self.open(Some(TRANSFER))?;
        let fields = [
            ("DeviceID", Field::Integer(i64::from(device))),
            // 포트는 네트워크 순서(큰 끝)로 담은 u16 을 작은 끝 정수로 읽은 값
            (
                "PortNumber",
                Field::Integer(i64::from(self.port.swap_bytes())),
            ),
        ];
        send(stream.as_mut(), 3, &request("Connect", &fields))?;
        match result(&receive(stream.as_mut())?)? {
            0 => {}
            2 => return Err(PortError::new("아이폰이 빠졌습니다")),
            3 => return Err(PortError::new("아이폰에서 SideDesk 앱을 열어 주세요")),
            number => return Err(PortError::new(format!("usbmuxd 가 거절함: {number}"))),
        }
        stream.set_read_timeout(Some(POLL)).map_err(broken)?;
        Ok(Box::new(MuxConn(stream)))
    }
}

struct MuxConn(Box<dyn MuxStream>);

impl Conn for MuxConn {
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

fn result(message: &Value) -> Result<i64, PortError> {
    if message.get("MessageType").and_then(Value::text) != Some("Result") {
        return Err(strange());
    }
    message
        .get("Number")
        .and_then(Value::integer)
        .ok_or_else(strange)
}

enum Field {
    Integer(i64),
}

/// 보내는 요청. 값은 고정 글자와 숫자뿐이라 글자를 따로 바꾸지 않는다.
fn request(kind: &str, fields: &[(&str, Field)]) -> String {
    let mut body = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n<dict>\n",
    );
    let mut add = |key: &str, value: String| {
        body.push_str(&format!("<key>{key}</key>{value}\n"));
    };
    add("MessageType", format!("<string>{kind}</string>"));
    add("ClientVersionString", "<string>sidedesk</string>".into());
    add("ProgName", "<string>sidedesk</string>".into());
    add("kLibUSBMuxVersion", "<integer>3</integer>".into());
    for (key, Field::Integer(value)) in fields {
        add(key, format!("<integer>{value}</integer>"));
    }
    body.push_str("</dict>\n</plist>\n");
    body
}

fn send(stream: &mut dyn MuxStream, tag: u32, body: &str) -> Result<(), PortError> {
    stream.write_all(&packet(tag, body)).map_err(broken)
}

fn packet(tag: u32, body: &str) -> Vec<u8> {
    let length = u32::try_from(HEADER + body.len()).unwrap_or(u32::MAX);
    let mut bytes = Vec::with_capacity(HEADER + body.len());
    for word in [length, VERSION, PLIST, tag] {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes.extend_from_slice(body.as_bytes());
    bytes
}

/// 메시지 하나. 머리와 본문 길이만큼만 읽는다(뒤따르는 앱 바이트를 삼키지 않게).
fn receive(stream: &mut dyn MuxStream) -> Result<Value, PortError> {
    let mut header = [0; HEADER];
    stream.read_exact(&mut header).map_err(broken)?;
    let word = |at: usize| {
        u32::from_le_bytes([header[at], header[at + 1], header[at + 2], header[at + 3]])
    };
    let length = word(0) as usize;
    if word(4) != VERSION || word(8) != PLIST || length < HEADER || length - HEADER > MAX_BODY {
        return Err(strange());
    }
    let mut body = vec![0; length - HEADER];
    stream.read_exact(&mut body).map_err(broken)?;
    let text = std::str::from_utf8(&body).map_err(|_| strange())?;
    plist(text).ok_or_else(strange)
}

/// usbmuxd 가 보내는 plist 값. 쓰지 않는 종류(실수, 날짜, 자료)는 `Other`.
#[derive(Clone, Debug, PartialEq)]
enum Value {
    Dict(Vec<(String, Value)>),
    Array(Vec<Value>),
    Text(String),
    Integer(i64),
    Bool(bool),
    Other,
}

impl Value {
    fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Self::Dict(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    fn text(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            _ => None,
        }
    }

    fn integer(&self) -> Option<i64> {
        match self {
            Self::Integer(number) => Some(*number),
            _ => None,
        }
    }

    fn array(&self) -> Option<&[Value]> {
        match self {
            Self::Array(items) => Some(items),
            _ => None,
        }
    }
}

const MAX_DEPTH: usize = 16;

/// XML plist 를 읽는다. usbmuxd 가 쓰는 만큼만 안다.
fn plist(text: &str) -> Option<Value> {
    let mut reader = Reader { rest: text };
    loop {
        let tag = reader.tag()?;
        if tag.name == "plist" && !tag.closing {
            break;
        }
    }
    reader.value(0)
}

struct Tag<'a> {
    name: &'a str,
    closing: bool,
    empty: bool,
}

struct Reader<'a> {
    rest: &'a str,
}

impl<'a> Reader<'a> {
    /// 다음 태그. 선언, 문서 형식, 주석은 건너뛴다. 태그 앞 글자는 버린다.
    fn tag(&mut self) -> Option<Tag<'a>> {
        loop {
            let start = self.rest.find('<')?;
            self.rest = &self.rest[start..];
            if let Some(after) = self.rest.strip_prefix("<!--") {
                let end = after.find("-->")?;
                self.rest = &after[end + 3..];
                continue;
            }
            let end = self.rest.find('>')?;
            let inner = &self.rest[1..end];
            self.rest = &self.rest[end + 1..];
            if inner.starts_with('?') || inner.starts_with('!') {
                continue;
            }
            let closing = inner.starts_with('/');
            let empty = inner.ends_with('/');
            let inner = inner.trim_start_matches('/').trim_end_matches('/');
            let name = inner.split_whitespace().next().unwrap_or("");
            return Some(Tag {
                name,
                closing,
                empty,
            });
        }
    }

    /// `</name>` 까지의 글자(엔티티를 풀기 전)
    fn raw_until(&mut self, name: &str) -> Option<&'a str> {
        let close = format!("</{name}>");
        let end = self.rest.find(&close)?;
        let raw = &self.rest[..end];
        self.rest = &self.rest[end + close.len()..];
        Some(raw)
    }

    fn value(&mut self, depth: usize) -> Option<Value> {
        let tag = self.tag()?;
        if tag.closing {
            return None;
        }
        self.value_of(tag, depth)
    }

    fn value_of(&mut self, tag: Tag<'a>, depth: usize) -> Option<Value> {
        if depth > MAX_DEPTH {
            return None;
        }
        if tag.empty {
            return Some(match tag.name {
                "dict" => Value::Dict(Vec::new()),
                "array" => Value::Array(Vec::new()),
                "string" => Value::Text(String::new()),
                "true" => Value::Bool(true),
                "false" => Value::Bool(false),
                _ => Value::Other,
            });
        }
        match tag.name {
            "dict" => {
                let mut entries = Vec::new();
                loop {
                    let key = self.tag()?;
                    if key.closing && key.name == "dict" {
                        return Some(Value::Dict(entries));
                    }
                    if key.name != "key" || key.closing {
                        return None;
                    }
                    let name = if key.empty {
                        String::new()
                    } else {
                        let raw = self.raw_until("key")?;
                        unescape(raw).unwrap_or_else(|| raw.to_owned())
                    };
                    let value = self.value(depth + 1)?;
                    entries.push((name, value));
                }
            }
            "array" => {
                let mut items = Vec::new();
                loop {
                    let next = self.tag()?;
                    if next.closing && next.name == "array" {
                        return Some(Value::Array(items));
                    }
                    if next.closing {
                        return None;
                    }
                    items.push(self.value_of(next, depth + 1)?);
                }
            }
            // 읽지 못하는 값 하나(모르는 엔티티, i64 를 넘는 수) 때문에 메시지 전체를 버리지 않는다.
            "string" => Some(unescape(self.raw_until("string")?).map_or(Value::Other, Value::Text)),
            "integer" => Some(
                self.raw_until("integer")?
                    .trim()
                    .parse()
                    .map_or(Value::Other, Value::Integer),
            ),
            name => {
                // 모르는 값은 닫는 태그까지 건너뛴다(같은 이름이 안에 겹치지 않는 값만 온다).
                self.raw_until(name)?;
                Some(Value::Other)
            }
        }
    }
}

fn unescape(raw: &str) -> Option<String> {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let end = rest[at..].find(';')? + at;
        out.push(match &rest[at + 1..end] {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            _ => return None,
        });
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    Some(out)
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
    use std::thread;

    use super::*;

    /// 메모리 통로 한쪽. 상대가 놓으면 읽기는 0 을 돌려준다.
    struct End {
        incoming: Receiver<Vec<u8>>,
        outgoing: Option<Sender<Vec<u8>>>,
        pending: VecDeque<u8>,
        timeout: Mutex<Option<Duration>>,
    }

    fn duplex() -> (End, End) {
        let (a_tx, a_rx) = mpsc::channel();
        let (b_tx, b_rx) = mpsc::channel();
        let end = |incoming, outgoing| End {
            incoming,
            outgoing: Some(outgoing),
            pending: VecDeque::new(),
            timeout: Mutex::new(None),
        };
        (end(a_rx, b_tx), end(b_rx, a_tx))
    }

    impl Read for End {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.pending.is_empty() {
                let wait = *self.timeout.lock().unwrap();
                let got = match wait {
                    Some(wait) => self.incoming.recv_timeout(wait),
                    None => self
                        .incoming
                        .recv()
                        .map_err(|_| RecvTimeoutError::Disconnected),
                };
                match got {
                    Ok(bytes) => self.pending.extend(bytes),
                    Err(RecvTimeoutError::Timeout) => return Err(ErrorKind::WouldBlock.into()),
                    Err(RecvTimeoutError::Disconnected) => return Ok(0),
                }
            }
            let count = buffer.len().min(self.pending.len());
            for (slot, byte) in buffer.iter_mut().zip(self.pending.drain(..count)) {
                *slot = byte;
            }
            Ok(count)
        }
    }

    impl Write for End {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let sender = self.outgoing.as_ref().ok_or(ErrorKind::BrokenPipe)?;
            sender
                .send(bytes.to_vec())
                .map_err(|_| io::Error::from(ErrorKind::BrokenPipe))?;
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl MuxStream for End {
        fn set_read_timeout(&self, time: Option<Duration>) -> io::Result<()> {
            *self.timeout.lock().unwrap() = time;
            Ok(())
        }
    }

    type Script = Box<dyn FnOnce(&mut End) + Send>;

    /// 접속마다 대본 하나를 꺼내 상대 쪽에서 돌린다. 대본이 없으면 접속 실패.
    struct FakeMuxd {
        scripts: Mutex<VecDeque<Script>>,
        asked: std::sync::Arc<Mutex<Vec<Value>>>,
    }

    impl FakeMuxd {
        fn new(scripts: Vec<Script>) -> Self {
            Self {
                scripts: Mutex::new(scripts.into()),
                asked: Default::default(),
            }
        }
    }

    impl Dial for FakeMuxd {
        fn open(&self) -> io::Result<Box<dyn MuxStream>> {
            let script = self
                .scripts
                .lock()
                .unwrap()
                .pop_front()
                .ok_or(ErrorKind::NotFound)?;
            let (mine, mut theirs) = duplex();
            let asked = std::sync::Arc::clone(&self.asked);
            thread::spawn(move || {
                theirs
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                if let Ok(request) = receive(&mut theirs) {
                    asked.lock().unwrap().push(request);
                }
                script(&mut theirs);
            });
            Ok(Box::new(mine))
        }
    }

    fn reply(end: &mut End, body: &str) {
        end.write_all(&packet(0, body)).unwrap();
    }

    fn wrap(dict: &str) -> String {
        format!(
            "<?xml version=\"1.0\"?><!DOCTYPE plist><plist version=\"1.0\"><dict>{dict}</dict></plist>"
        )
    }

    fn result_body(number: i64) -> String {
        wrap(&format!(
            "<key>MessageType</key><string>Result</string><key>Number</key><integer>{number}</integer>"
        ))
    }

    fn device(id: u32, serial: &str, kind: &str) -> String {
        format!(
            "<dict><key>DeviceID</key><integer>{id}</integer><key>MessageType</key><string>Attached</string>\
             <key>Properties</key><dict><key>ConnectionType</key><string>{kind}</string>\
             <key>DeviceID</key><integer>{id}</integer><key>SerialNumber</key><string>{serial}</string></dict></dict>"
        )
    }

    fn list_body(devices: &[String]) -> String {
        wrap(&format!(
            "<key>DeviceList</key><array>{}</array>",
            devices.concat()
        ))
    }

    fn event(device_xml: &str) -> String {
        // device() 는 dict 하나라 그대로 plist 로 감싼다.
        format!("<?xml version=\"1.0\"?><plist version=\"1.0\">{device_xml}</plist>")
    }

    fn links(scripts: Vec<Script>) -> UsbmuxLinks {
        UsbmuxLinks::new(Box::new(FakeMuxd::new(scripts)), 47_381)
    }

    fn ids(phones: &[Phone]) -> Vec<String> {
        phones.iter().map(|phone| phone.id.clone()).collect()
    }

    #[test]
    fn reads_nested_plists_entities_and_skips_unknown_values() {
        let text = "<?xml version=\"1.0\"?>\n<!-- 주석 -->\n<plist version=\"1.0\"><dict>\
            <key>A&amp;B</key><string>&lt;x&gt; &quot;y&quot; &apos;z&apos;</string>\
            <key>N</key><integer> -3 </integer>\
            <key>R</key><real>1.5</real>\
            <key>D</key><data>AAEC</data>\
            <key>E</key><string/>\
            <key>T</key><true/>\
            <key>L</key><array><dict/><array/><integer>7</integer></array>\
            </dict></plist>";
        let value = plist(text).unwrap();
        assert_eq!(
            value.get("A&B").and_then(Value::text),
            Some("<x> \"y\" 'z'")
        );
        assert_eq!(value.get("N").and_then(Value::integer), Some(-3));
        assert_eq!(value.get("R"), Some(&Value::Other));
        assert_eq!(value.get("D"), Some(&Value::Other));
        assert_eq!(value.get("E").and_then(Value::text), Some(""));
        assert_eq!(value.get("T"), Some(&Value::Bool(true)));
        assert_eq!(
            value.get("L").and_then(Value::array).map(<[Value]>::len),
            Some(3)
        );
    }

    #[test]
    fn one_unreadable_value_does_not_spoil_the_message() {
        let text = "<plist><dict><key>Big</key><integer>99999999999999999999</integer>\
            <key>Odd</key><string>&#65;</string><key>N</key><integer>4</integer></dict></plist>";
        let value = plist(text).unwrap();
        assert_eq!(value.get("Big"), Some(&Value::Other));
        assert_eq!(value.get("Odd"), Some(&Value::Other));
        assert_eq!(value.get("N").and_then(Value::integer), Some(4));
    }

    #[test]
    fn refuses_broken_or_too_deep_plists() {
        assert!(plist("<plist><dict><key>a</key>").is_none());
        assert!(plist("<plist><dict><key>a</key><string>x</string></plist>").is_none());
        assert!(plist("<plist><dict><string>a</string></dict></plist>").is_none());
        let deep = format!(
            "<plist>{}{}</plist>",
            "<array>".repeat(20),
            "</array>".repeat(20)
        );
        assert!(plist(&deep).is_none());
    }

    #[test]
    fn requests_carry_the_port_in_network_order() {
        let body = request(
            "Connect",
            &[(
                "PortNumber",
                Field::Integer(i64::from(47_381u16.swap_bytes())),
            )],
        );
        let value = plist(&body).unwrap();
        assert_eq!(
            value.get("MessageType").and_then(Value::text),
            Some("Connect")
        );
        let port = value.get("PortNumber").and_then(Value::integer).unwrap();
        assert_eq!(
            u16::try_from(port).unwrap().to_le_bytes(),
            47_381u16.to_be_bytes()
        );
    }

    #[test]
    fn lists_only_usb_iphones_once_each() {
        let body = list_body(&[
            device(1, "AAA", "USB"),
            device(2, "BBB", "Network"),
            // 같은 아이폰이 USB 와 와이파이로 둘 다 보임
            device(3, "AAA", "Network"),
        ]);
        let links = links(vec![Box::new(move |end| reply(end, &body))]);
        let phones = links.attached().unwrap();
        assert_eq!(ids(&phones), ["ios:AAA"]);
        assert_eq!(phones[0].link, Link::Usb);
        assert_eq!(phones[0].model.as_deref(), Some("iPhone"));
    }

    #[test]
    fn watch_starts_from_the_current_list_and_follows_events() {
        let list = list_body(&[device(1, "AAA", "USB")]);
        let events = vec![
            result_body(0),
            // Listen 직후 이미 꽂힌 기기도 다시 온다.
            event(&device(1, "AAA", "USB")),
            event(&device(5, "CCC", "USB")),
            wrap(
                "<key>MessageType</key><string>Paired</string><key>DeviceID</key><integer>5</integer>",
            ),
            wrap(
                "<key>MessageType</key><string>Detached</string><key>DeviceID</key><integer>1</integer>",
            ),
        ];
        let links = links(vec![
            Box::new(move |end| reply(end, &list)),
            Box::new(move |end| {
                for body in &events {
                    reply(end, body);
                }
            }),
        ]);
        let mut seen = Vec::new();
        let ended = links.watch(&mut |phones| seen.push(ids(&phones)));
        assert!(ended.is_err(), "통로가 닫히면 돌아온다");
        assert_eq!(
            seen,
            [
                vec!["ios:AAA".to_owned()],
                vec!["ios:AAA".to_owned(), "ios:CCC".to_owned()],
                vec!["ios:CCC".to_owned()],
            ],
            "빈 목록을 먼저 내지 않고, 모르는 알림은 건너뜀"
        );
    }

    #[test]
    fn connects_and_then_talks_to_the_app() {
        let list = list_body(&[device(7, "AAA", "USB")]);
        let (done, finished) = mpsc::channel();
        let muxd = FakeMuxd::new(vec![
            Box::new(move |end| reply(end, &list)),
            Box::new(move |end| {
                // 결과 바로 뒤에 앱 바이트가 붙어 와도 삼키지 않아야 한다.
                let mut bytes = packet(0, &result_body(0));
                bytes.extend_from_slice(b"pong");
                end.write_all(&bytes).unwrap();
                let mut got = [0; 4];
                end.read_exact(&mut got).unwrap();
                done.send(got).unwrap();
            }),
        ]);
        let asked = std::sync::Arc::clone(&muxd.asked);
        let links = UsbmuxLinks::new(Box::new(muxd), 47_381);
        links.attached().unwrap();
        let mut conn = links.connect("ios:AAA").unwrap();
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
        assert_eq!(&finished.recv().unwrap(), b"ping");
        let asked = asked.lock().unwrap();
        let connect = &asked[1];
        assert_eq!(
            connect.get("MessageType").and_then(Value::text),
            Some("Connect")
        );
        assert_eq!(connect.get("DeviceID").and_then(Value::integer), Some(7));
    }

    #[test]
    fn explains_each_refusal() {
        for (number, words) in [(3, "앱을 열어"), (2, "빠졌습니다"), (5, "거절함: 5")] {
            let list = list_body(&[device(7, "AAA", "USB")]);
            let answer = result_body(number);
            let links = links(vec![
                Box::new(move |end| reply(end, &list)),
                Box::new(move |end| reply(end, &answer)),
            ]);
            links.attached().unwrap();
            let error = links.connect("ios:AAA").err().unwrap();
            assert!(error.0.contains(words), "{number}: {}", error.0);
        }
        let links = links(Vec::new());
        assert!(
            links
                .connect("ios:AAA")
                .err()
                .unwrap()
                .0
                .contains("보이지 않습니다")
        );
        assert!(
            links
                .attached()
                .unwrap_err()
                .0
                .contains("usbmuxd 에 접속하지 못함")
        );
    }

    #[test]
    fn reports_strange_headers_and_cut_messages() {
        let bad_version = {
            let mut bytes = packet(0, &list_body(&[]));
            bytes[4] = 2;
            bytes
        };
        let too_long = {
            let mut bytes = packet(0, "");
            bytes[..4].copy_from_slice(&u32::MAX.to_le_bytes());
            bytes
        };
        let cut = packet(0, &list_body(&[]))[..20].to_vec();
        for bytes in [bad_version, too_long, cut, b"short".to_vec()] {
            let links = links(vec![Box::new(move |end| end.write_all(&bytes).unwrap())]);
            assert!(links.attached().is_err());
        }
        let not_a_list = wrap("<key>MessageType</key><string>Result</string>");
        let links = links(vec![Box::new(move |end| reply(end, &not_a_list))]);
        assert!(links.attached().unwrap_err().0.contains("이상함"));
    }
}

#[cfg(test)]
mod tcp_dial_tests {
    use super::*;

    #[test]
    fn tcp_dial_reaches_a_listening_service_and_reports_a_missing_one() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        assert!(TcpDial(address).open().is_ok());
        drop(listener);
        assert!(TcpDial(address).open().is_err());
        assert_eq!(TcpDial::default().0.port(), 27015);
    }
}
