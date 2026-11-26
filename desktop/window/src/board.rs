//! 화면에 보일 상태. 연결 스레드는 `Report`, `Prompt` 로 알리고, 화면은 명령을 보낸다.

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use sidedesk_core::{KEY_LEN, beacon};
use sidedesk_domain::{Keys, Link, Note, PairRequests, Phone, Prompt, Report};

use crate::json;

/// 짝 맺기를 누른 뒤 숫자가 뜨기까지 기다리는 시간. 숫자가 뜬 뒤의 60초는 연결 쪽이 센다.
const CODE_WAIT: Duration = Duration::from_secs(60);

/// 정보 칸에 보일 이 컴퓨터의 값
pub struct Computer {
    pub name: String,
    pub version: String,
    pub key: [u8; KEY_LEN],
}

#[derive(Clone, Debug, PartialEq)]
enum Status {
    Connected { key: [u8; KEY_LEN] },
    Lost(String),
    Refused(String),
}

struct Pairing {
    /// 같은 폰을 다시 눌렀을 때 이전 시간 제한이 새 짝 맺기를 닫지 않게 한다.
    round: u64,
    id: String,
    code: Option<String>,
    reply: Option<Sender<bool>>,
}

struct Notice {
    text: String,
    good: bool,
}

#[derive(Default)]
struct State {
    attached: Vec<Phone>,
    status: HashMap<String, Status>,
    pairing: Option<Pairing>,
    rounds: u64,
    notice: Option<Notice>,
    /// 키마다 마지막으로 그 키로 붙었던 폰
    seen: HashMap<[u8; KEY_LEN], String>,
    /// 링크 자체의 문제(adb 없음, 블루투스 꺼짐)
    problems: HashMap<Link, String>,
}

/// 화면에 JSON 을 보내는 함수
type Listener = Box<dyn Fn(String) + Send>;

pub struct Board {
    state: Mutex<State>,
    computer: Computer,
    keys: &'static dyn Keys,
    requests: &'static PairRequests,
    changed: Mutex<Option<Listener>>,
    code_wait: Duration,
}

impl Board {
    pub fn new(
        computer: Computer,
        keys: &'static dyn Keys,
        requests: &'static PairRequests,
    ) -> Self {
        Self {
            state: Mutex::default(),
            computer,
            keys,
            requests,
            changed: Mutex::new(None),
            code_wait: CODE_WAIT,
        }
    }

    /// 상태가 바뀔 때마다 화면에 보낼 JSON 을 받는다.
    pub fn on_change(&self, send: impl Fn(String) + Send + 'static) {
        *self.changed.lock().unwrap_or_else(|lock| lock.into_inner()) = Some(Box::new(send));
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|lock| lock.into_inner())
    }

    /// 화면에서 온 명령
    pub fn command(&'static self, body: &str) {
        let Some((name, value)) = json::command(body) else {
            return;
        };
        match name.as_str() {
            "ready" => {}
            "pair" => self.start_pairing(&value),
            "answer" => {
                let mut state = self.lock();
                let same = value == "true";
                if let Some(pairing) = &mut state.pairing
                    && let Some(reply) = pairing.reply.take()
                {
                    let _ = reply.send(same);
                }
                if !same {
                    state.pairing = None;
                }
            }
            "forget" => {
                if let Some(key) = decode(&value)
                    && let Err(error) = self.keys.forget(key)
                {
                    self.lock().notice = Some(Notice {
                        text: error.0,
                        good: false,
                    });
                }
            }
            _ => return,
        }
        self.publish();
    }

    fn start_pairing(&'static self, id: &str) {
        let round = {
            let mut state = self.lock();
            // 등록은 USB 로만 한다.
            if state.pairing.is_some()
                || !state
                    .attached
                    .iter()
                    .any(|phone| phone.id == id && phone.link == Link::Usb)
            {
                return;
            }
            state.rounds += 1;
            let round = state.rounds;
            state.notice = None;
            state.pairing = Some(Pairing {
                round,
                id: id.to_owned(),
                code: None,
                reply: None,
            });
            round
        };
        self.requests.ask(id);
        // 앱이 끝내 안 열리면 상자를 닫는다.
        let wait = self.code_wait;
        thread::spawn(move || {
            thread::sleep(wait);
            let expired = {
                let mut state = self.lock();
                let expired = state
                    .pairing
                    .as_ref()
                    .is_some_and(|p| p.round == round && p.code.is_none());
                if expired {
                    let id = state.pairing.take().map(|p| p.id).unwrap_or_default();
                    self.requests.take(&id);
                    state.notice = Some(Notice {
                        text: "등록하지 못함: 기기에서 SideDesk 앱이 열리지 않음".to_owned(),
                        good: false,
                    });
                }
                expired
            };
            if expired {
                self.publish();
            }
        });
    }

    fn publish(&self) {
        let snapshot = self.snapshot();
        if let Some(send) = self
            .changed
            .lock()
            .unwrap_or_else(|lock| lock.into_inner())
            .as_ref()
        {
            send(snapshot);
        }
    }

    /// 화면 전체 상태를 JSON 으로
    pub fn snapshot(&self) -> String {
        let (trusted, trust_error) = match self.keys.trusted() {
            Ok(trusted) => (trusted, None),
            Err(error) => (Vec::new(), Some(error.0)),
        };
        let state = self.lock();
        let paired = trusted.iter().map(|paired| {
            // 붙었던 기록이 없으면 저장한 일련번호(USB)나 알림 신호 지문(블루투스)으로 찾는다.
            let mark = beacon(&paired.key);
            let id = state
                .seen
                .get(&paired.key)
                .filter(|id| state.attached.iter().any(|phone| &phone.id == *id))
                .or_else(|| {
                    state
                        .attached
                        .iter()
                        .find(|phone| match phone.link {
                            Link::Usb => !paired.serial.is_empty() && phone.id == paired.serial,
                            Link::Ble => phone.beacon == Some(mark),
                        })
                        .map(|phone| &phone.id)
                })
                .or_else(|| state.seen.get(&paired.key));
            let plugged = id.and_then(|id| state.attached.iter().find(|phone| phone.id == *id));
            // 이름 없이 저장된 기기는 꽂혀 있는 동안 adb 가 알려 준 모델명을 쓴다.
            let name = match (paired.name.as_str(), plugged) {
                ("", Some(phone)) => phone.model.as_deref().unwrap_or(""),
                (name, _) => name,
            };
            let (status, reason) = match id.and_then(|id| state.status.get(id)) {
                Some(Status::Connected { key }) if *key == paired.key => ("connected", None),
                Some(Status::Lost(reason)) => ("lost", Some(reason.as_str())),
                Some(Status::Refused(reason)) => ("refused", Some(reason.as_str())),
                _ if plugged.is_some() => ("waiting", None),
                _ => ("idle", None),
            };
            let serial = plugged
                .filter(|phone| phone.link == Link::Usb)
                .map(|phone| phone.id.as_str())
                .or((!paired.serial.is_empty()).then_some(paired.serial.as_str()));
            let link = plugged.map(|phone| phone.link.to_string());
            json::object(&[
                ("key", json::text(&encode(&paired.key))),
                ("fingerprint", json::text(&fingerprint(&paired.key))),
                ("name", json::text(name)),
                ("link", json::optional(link.as_deref())),
                ("serial", json::optional(serial)),
                ("status", json::text(status)),
                ("reason", json::optional(reason)),
            ])
        });
        // 블루투스 기기는 등록된 것만 위 등록 줄에 보인다.
        let attached = state
            .attached
            .iter()
            .filter(|phone| phone.link == Link::Usb)
            .map(|phone| {
                let (status, reason) = match (&phone.blocked, state.status.get(&phone.id)) {
                    (Some(reason), _) => ("blocked", Some(reason.as_str())),
                    (None, Some(Status::Connected { .. })) => ("connected", None),
                    (None, Some(Status::Refused(reason))) => ("refused", Some(reason.as_str())),
                    (None, Some(Status::Lost(reason))) => ("waiting", Some(reason.as_str())),
                    (None, None) => ("waiting", None),
                };
                let status = if state.pairing.as_ref().is_some_and(|p| p.id == phone.id) {
                    "pairing"
                } else {
                    status
                };
                // 이 기기로 붙었던 키나 이 일련번호가 등록 목록에 있으면 등록된 기기다.
                // 일련번호만 맞고 거절됐다면(앱을 다시 깔아 키가 바뀜) 다시 등록할 수 있게 둔다.
                let refused = matches!(state.status.get(&phone.id), Some(Status::Refused(_)));
                let paired = state.seen.iter().any(|(key, id)| {
                    *id == phone.id && trusted.iter().any(|paired| paired.key == *key)
                }) || (!refused
                    && trusted.iter().any(|paired| paired.serial == phone.id));
                json::object(&[
                    ("id", json::text(&phone.id)),
                    (
                        "name",
                        json::text(phone.model.as_deref().unwrap_or(&phone.id)),
                    ),
                    ("status", json::text(status)),
                    ("reason", json::optional(reason)),
                    ("paired", paired.to_string()),
                ])
            });
        let pairing = state.pairing.as_ref().map_or_else(
            || "null".to_owned(),
            |pairing| {
                let name = state
                    .attached
                    .iter()
                    .find(|phone| phone.id == pairing.id)
                    .and_then(|phone| phone.model.clone())
                    .unwrap_or_else(|| pairing.id.clone());
                json::object(&[
                    ("id", json::text(&pairing.id)),
                    ("name", json::text(&name)),
                    ("code", json::optional(pairing.code.as_deref())),
                    (
                        "waiting",
                        (pairing.code.is_some() && pairing.reply.is_none()).to_string(),
                    ),
                ])
            },
        );
        let notice = match (&trust_error, &state.notice) {
            (Some(error), _) => {
                json::object(&[("text", json::text(error)), ("good", "false".into())])
            }
            (None, Some(notice)) => json::object(&[
                ("text", json::text(&notice.text)),
                ("good", notice.good.to_string()),
            ]),
            (None, None) => "null".to_owned(),
        };
        json::object(&[
            (
                "computer",
                json::object(&[
                    ("name", json::text(&self.computer.name)),
                    ("version", json::text(&self.computer.version)),
                    ("fingerprint", json::text(&fingerprint(&self.computer.key))),
                ]),
            ),
            ("paired", json::array(paired)),
            ("attached", json::array(attached)),
            ("pairing", pairing),
            ("notice", notice),
            (
                "problems",
                json::array(
                    [Link::Usb, Link::Ble]
                        .into_iter()
                        .filter_map(|link| state.problems.get(&link).map(|text| json::text(text))),
                ),
            ),
        ])
    }

    /// 짝 맺기 중인 폰이 끝났으면 상자를 닫고 이유를 남긴다.
    fn end_pairing(state: &mut State, id: &str, notice: Option<Notice>) {
        if state
            .pairing
            .as_ref()
            .is_some_and(|pairing| pairing.id == id)
        {
            state.pairing = None;
            state.notice = notice;
        }
    }

    /// 숫자가 뜬 뒤에 끝난 짝 맺기만 실패로 닫는다. 숫자 전에는 앱이 열리길 기다리는 중이다.
    fn fail_pairing(state: &mut State, id: &str, reason: &str) {
        if state
            .pairing
            .as_ref()
            .is_some_and(|pairing| pairing.id == id && pairing.code.is_some())
        {
            Self::end_pairing(
                state,
                id,
                Some(Notice {
                    text: format!("등록하지 못함: {reason}"),
                    good: false,
                }),
            );
        }
    }
}

impl Report for Board {
    fn note(&self, phone: &str, note: Note) {
        {
            let mut state = self.lock();
            match note {
                Note::Connected { key } => {
                    state.seen.insert(key, phone.to_owned());
                    state
                        .status
                        .insert(phone.to_owned(), Status::Connected { key });
                }
                Note::Paired { key } => {
                    state.seen.insert(key, phone.to_owned());
                    Self::end_pairing(
                        &mut state,
                        phone,
                        Some(Notice {
                            text: "등록했습니다".to_owned(),
                            good: true,
                        }),
                    );
                }
                Note::Lost(reason) => {
                    Self::fail_pairing(&mut state, phone, &reason);
                    state.status.insert(phone.to_owned(), Status::Lost(reason));
                }
                Note::Refused(reason) => {
                    Self::fail_pairing(&mut state, phone, &reason);
                    state
                        .status
                        .insert(phone.to_owned(), Status::Refused(reason));
                }
            }
        }
        self.publish();
    }

    fn link(&self, link: Link, problem: Option<String>) {
        {
            let mut state = self.lock();
            match problem {
                None => {
                    if state.problems.remove(&link).is_none() {
                        return;
                    }
                }
                // 링크를 쓸 수 없으면 그 링크의 기기를 알 수 없다. 그 이유로 끊김으로 둔다.
                Some(reason) => {
                    if state.problems.get(&link) == Some(&reason) {
                        return;
                    }
                    let ids: Vec<String> = state
                        .attached
                        .iter()
                        .filter(|phone| phone.link == link)
                        .map(|phone| phone.id.clone())
                        .collect();
                    for id in &ids {
                        state
                            .status
                            .insert(id.clone(), Status::Lost(reason.clone()));
                    }
                    state.attached.retain(|phone| phone.link != link);
                    if let Some(id) = state.pairing.as_ref().map(|pairing| pairing.id.clone())
                        && ids.contains(&id)
                    {
                        self.requests.take(&id);
                        state.pairing = None;
                    }
                    state.problems.insert(link, reason);
                }
            }
        }
        self.publish();
    }

    fn phones(&self, link: Link, phones: &[Phone]) {
        {
            let mut state = self.lock();
            let gone: Vec<String> = state
                .attached
                .iter()
                .filter(|phone| phone.link == link)
                .filter(|old| !phones.iter().any(|phone| phone.id == old.id))
                .map(|phone| phone.id.clone())
                .collect();
            let reason = match link {
                Link::Usb => "USB 연결이 끊김",
                Link::Ble => "블루투스 신호가 사라짐",
            };
            for id in gone {
                // 블루투스 id 는 주소가 바뀔 때마다 새로 생기므로, 등록 줄이 가리키지 않는 옛 id 는 지운다.
                let pointed = state.seen.values().any(|seen| *seen == id);
                if link == Link::Ble && !pointed {
                    state.status.remove(&id);
                } else {
                    state.status.insert(id, Status::Lost(reason.to_owned()));
                }
            }
            if let Some(pairing) = &state.pairing
                && !phones.iter().any(|phone| phone.id == pairing.id)
            {
                let id = pairing.id.clone();
                self.requests.take(&id);
                Self::end_pairing(
                    &mut state,
                    &id,
                    Some(Notice {
                        text: "등록하지 못함: USB 연결이 끊김".to_owned(),
                        good: false,
                    }),
                );
            }
            state.attached.retain(|phone| phone.link != link);
            state.attached.extend_from_slice(phones);
        }
        self.publish();
    }
}

impl Prompt for Board {
    fn ask(&self, phone: &str, code: &str) -> Receiver<bool> {
        let (reply, receiver) = mpsc::channel();
        {
            let mut state = self.lock();
            state.notice = None;
            state.rounds += 1;
            let round = state.rounds;
            state.pairing = Some(Pairing {
                round,
                id: phone.to_owned(),
                code: Some(code.to_owned()),
                reply: Some(reply),
            });
        }
        self.publish();
        receiver
    }
}

fn encode(key: &[u8]) -> String {
    key.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn decode(text: &str) -> Option<[u8; KEY_LEN]> {
    if text.len() != KEY_LEN * 2 || !text.is_ascii() {
        return None;
    }
    let bytes: Option<Vec<u8>> = (0..KEY_LEN)
        .map(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).ok())
        .collect();
    bytes?.try_into().ok()
}

/// 사람이 비교하기 쉽게 앞 8바이트를 넷씩 끊는다.
fn fingerprint(key: &[u8]) -> String {
    key[..8].chunks(2).map(encode).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex as StdMutex;

    use sidedesk_core::Identity;
    use sidedesk_domain::{Paired, PortError};

    use super::*;

    #[derive(Default)]
    struct MemoryKeys(StdMutex<Vec<Paired>>);

    impl Keys for MemoryKeys {
        fn identity(&self) -> Result<Identity, PortError> {
            Ok(Identity::generate())
        }

        fn trusted(&self) -> Result<Vec<Paired>, PortError> {
            Ok(self.0.lock().unwrap().clone())
        }

        fn trust(&self, key: [u8; KEY_LEN], name: &str, serial: &str) -> Result<(), PortError> {
            self.0.lock().unwrap().push(Paired {
                key,
                name: name.to_owned(),
                serial: serial.to_owned(),
            });
            Ok(())
        }

        fn update(&self, key: [u8; KEY_LEN], name: &str, serial: &str) -> Result<(), PortError> {
            if let Some(saved) = self.0.lock().unwrap().iter_mut().find(|p| p.key == key) {
                saved.name = name.to_owned();
                saved.serial = serial.to_owned();
            }
            Ok(())
        }

        fn forget(&self, key: [u8; KEY_LEN]) -> Result<(), PortError> {
            self.0.lock().unwrap().retain(|paired| paired.key != key);
            Ok(())
        }
    }

    fn board_waiting(
        code_wait: Duration,
    ) -> (&'static Board, &'static MemoryKeys, &'static PairRequests) {
        let keys: &'static MemoryKeys = Box::leak(Box::default());
        let requests: &'static PairRequests = Box::leak(Box::default());
        let computer = Computer {
            name: "맥".into(),
            version: "0.1.0".into(),
            key: [7; KEY_LEN],
        };
        let mut board = Board::new(computer, keys, requests);
        board.code_wait = code_wait;
        (Box::leak(Box::new(board)), keys, requests)
    }

    fn board() -> (&'static Board, &'static MemoryKeys, &'static PairRequests) {
        board_waiting(CODE_WAIT)
    }

    fn phone(id: &str) -> Phone {
        Phone::usb(id, Some("SM A716S".into()), None)
    }

    fn air(key: [u8; KEY_LEN]) -> Phone {
        Phone {
            id: "ble:1".into(),
            model: Some("Galaxy A71".into()),
            blocked: None,
            link: Link::Ble,
            beacon: Some(beacon(&key)),
        }
    }

    #[test]
    fn pairing_from_the_window() {
        let (board, keys, requests) = board();
        board.phones(Link::Usb, &[phone("a")]);
        board.note("a", Note::Refused("등록되지 않은 상대".into()));
        assert!(board.snapshot().contains(r#""status":"refused""#));

        board.command(r#"{"pair":"a"}"#);
        assert!(requests.is_asked("a"));
        assert!(
            board
                .snapshot()
                .contains(r#""pairing":{"id":"a","name":"SM A716S","code":null"#)
        );

        let reply = board.ask("a", "123456");
        assert!(
            board
                .snapshot()
                .contains(r#""code":"123456","waiting":false"#)
        );
        board.command(r#"{"answer":true}"#);
        assert_eq!(reply.try_recv(), Ok(true));
        assert!(board.snapshot().contains(r#""waiting":true"#));

        keys.trust([1; KEY_LEN], "SM A716S", "a").unwrap();
        board.note("a", Note::Paired { key: [1; KEY_LEN] });
        let snapshot = board.snapshot();
        assert!(snapshot.contains(r#""pairing":null"#), "{snapshot}");
        assert!(snapshot.contains(r#""good":true"#), "{snapshot}");
        // 짝을 맺은 뒤 다시 붙다 끊겨도 실패로 보이지 않는다.
        board.note("a", Note::Lost("끊김".into()));
        assert!(board.snapshot().contains("등록했습니다"));
        board.note("a", Note::Connected { key: [1; KEY_LEN] });
        let snapshot = board.snapshot();
        assert!(
            snapshot.contains(r#""name":"SM A716S","status":"connected""#),
            "{snapshot}"
        );
    }

    #[test]
    fn a_failed_pairing_closes_the_box_with_a_reason() {
        let (board, _, _) = board();
        board.phones(Link::Usb, &[phone("a")]);
        board.command(r#"{"pair":"a"}"#);
        let _reply = board.ask("a", "123456");
        board.note("a", Note::Refused("등록을 취소함".into()));
        let snapshot = board.snapshot();
        assert!(snapshot.contains(r#""pairing":null"#));
        assert!(snapshot.contains("등록을 취소함"));
    }

    #[test]
    fn the_box_waits_for_the_app_then_gives_up() {
        let (board, _, requests) = board_waiting(Duration::from_millis(100));
        board.phones(Link::Usb, &[phone("a")]);
        board.command(r#"{"pair":"a"}"#);
        board.note("a", Note::Lost("기기에서 SideDesk 앱을 열어 주세요".into()));
        board.note("a", Note::Refused("등록되지 않은 상대".into()));
        assert!(
            board.snapshot().contains(r#""pairing":{"#),
            "숫자 전에는 닫지 않는다"
        );
        thread::sleep(Duration::from_millis(250));
        let snapshot = board.snapshot();
        assert!(snapshot.contains(r#""pairing":null"#), "{snapshot}");
        assert!(snapshot.contains("앱이 열리지 않음"));
        assert!(!requests.is_asked("a"));
    }

    #[test]
    fn unplugging_turns_the_phone_red_and_ends_pairing() {
        let (board, keys, requests) = board();
        keys.trust([1; KEY_LEN], "SM A716S", "a").unwrap();
        board.phones(Link::Usb, &[phone("a"), phone("b")]);
        board.note("a", Note::Connected { key: [1; KEY_LEN] });
        board.command(r#"{"pair":"b"}"#);
        board.phones(Link::Usb, &[]);
        let snapshot = board.snapshot();
        assert!(
            snapshot.contains(r#""status":"lost","reason":"USB 연결이 끊김""#),
            "{snapshot}"
        );
        assert!(snapshot.contains(r#""pairing":null"#));
        assert!(!requests.is_asked("b"));
    }

    #[test]
    fn a_paired_phone_offers_no_pairing_while_disconnected() {
        let (board, keys, _) = board();
        keys.trust([1; KEY_LEN], "SM A716S", "a").unwrap();
        board.phones(Link::Usb, &[phone("a")]);
        board.note("a", Note::Connected { key: [1; KEY_LEN] });
        board.note("a", Note::Lost("기기에서 SideDesk 앱을 열어 주세요".into()));
        assert!(board.snapshot().contains(r#""paired":true"#));
    }

    #[test]
    fn saying_no_closes_the_box_and_only_one_pairing_at_a_time() {
        let (board, _, requests) = board();
        board.phones(Link::Usb, &[phone("a"), phone("b")]);
        board.command(r#"{"pair":"a"}"#);
        board.command(r#"{"pair":"b"}"#);
        assert!(!requests.is_asked("b"));
        let reply = board.ask("a", "123456");
        board.command(r#"{"answer":false}"#);
        assert_eq!(reply.try_recv(), Ok(false));
        assert!(board.snapshot().contains(r#""pairing":null"#));
    }

    #[test]
    fn losing_adb_shows_its_reason_on_every_phone() {
        let (board, keys, _) = board();
        keys.trust([1; KEY_LEN], "SM A716S", "a").unwrap();
        board.phones(Link::Usb, &[phone("a")]);
        board.note("a", Note::Connected { key: [1; KEY_LEN] });
        board.link(Link::Usb, Some("adb 서버에 접속하지 못함".into()));
        let snapshot = board.snapshot();
        assert!(
            snapshot.contains(r#""status":"lost","reason":"adb 서버에 접속하지 못함""#),
            "{snapshot}"
        );
        assert!(snapshot.contains(r#""attached":[]"#));
        assert!(snapshot.contains(r#""problems":["adb 서버에 접속하지 못함"]"#));
        board.link(Link::Usb, None);
        assert!(board.snapshot().contains(r#""problems":[]"#));
    }

    #[test]
    fn a_registered_phone_shows_bluetooth_and_strangers_stay_hidden() {
        let (board, keys, _) = board();
        keys.trust([1; KEY_LEN], "SM A716S", "a").unwrap();
        board.phones(
            Link::Ble,
            &[
                air([1; KEY_LEN]),
                Phone {
                    id: "ble:2".into(),
                    ..air([9; KEY_LEN])
                },
            ],
        );
        let snapshot = board.snapshot();
        assert!(snapshot.contains(r#""link":"블루투스""#), "{snapshot}");
        assert!(
            snapshot.contains(r#""serial":"a""#),
            "블루투스여도 저장한 일련번호를 보임"
        );
        assert!(
            snapshot.contains(r#""attached":[]"#),
            "등록 안 된 블루투스 기기는 숨김"
        );
    }

    #[test]
    fn bluetooth_list_does_not_wipe_usb() {
        let (board, keys, _) = board();
        keys.trust([1; KEY_LEN], "SM A716S", "a").unwrap();
        board.phones(Link::Usb, &[phone("a")]);
        board.note("a", Note::Connected { key: [1; KEY_LEN] });
        board.phones(Link::Ble, &[]);
        board.link(Link::Ble, Some("맥의 블루투스가 꺼져 있습니다".into()));
        let snapshot = board.snapshot();
        assert!(snapshot.contains(r#""link":"USB""#), "{snapshot}");
        assert!(snapshot.contains(r#""status":"connected""#), "{snapshot}");
    }

    #[test]
    fn a_late_bluetooth_drop_does_not_cover_usb() {
        let (board, keys, _) = board();
        keys.trust([1; KEY_LEN], "SM A716S", "a").unwrap();
        board.phones(Link::Usb, &[phone("a")]);
        board.phones(Link::Ble, &[air([1; KEY_LEN])]);
        board.note("a", Note::Connected { key: [1; KEY_LEN] });
        // 물러난 블루투스는 알리지 않으므로, 끊김이 와도 다른 id 에 붙는다.
        board.note("ble:1", Note::Lost("끊김".into()));
        assert!(board.snapshot().contains(r#""status":"connected""#));
    }

    #[test]
    fn forgetting_from_the_window() {
        let (board, keys, _) = board();
        keys.trust([1; KEY_LEN], "A", "a").unwrap();
        board.command(&format!(r#"{{"forget":"{}"}}"#, encode(&[1; KEY_LEN])));
        assert!(keys.trusted().unwrap().is_empty());
    }
}
