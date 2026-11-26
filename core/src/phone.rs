//! 폰 앱(Kotlin, Swift)이 쓰는 연결.

use std::sync::{Arc, Mutex, MutexGuard};

use crate::{
    Body, CommandResult, Event, Identity, KEY_LEN, KillProcess, Location, Performance, Process,
    Processes, Role, Session, SessionError, Subscribe, place_tenths,
};

#[derive(uniffi::Record)]
pub struct KeyPair {
    pub private_key: Vec<u8>,
    pub public_key: Vec<u8>,
}

#[uniffi::export]
pub fn generate_key_pair() -> KeyPair {
    let identity = Identity::generate();
    KeyPair {
        private_key: identity.private.to_vec(),
        public_key: identity.public.to_vec(),
    }
}

#[derive(Debug, PartialEq, uniffi::Enum)]
pub enum PhoneEvent {
    PairingCode {
        code: String,
    },
    Paired {
        computer_key: Vec<u8>,
    },
    Ready {
        computer_key: Vec<u8>,
    },
    ComputerName {
        name: String,
    },
    Performance {
        value: PerformanceInfo,
    },
    Processes {
        items: Vec<ProcessInfo>,
    },
    CommandResult {
        request_id: u64,
        ok: bool,
        reason: String,
    },
    Weather {
        value: WeatherInfo,
    },
    AiSessions {
        items: Vec<AiSessionInfo>,
    },
    AiAlert {
        value: AiAlertInfo,
    },
    Forge {
        github: Option<ForgeInfo>,
        gitlab: Option<ForgeInfo>,
    },
    /// book 은 이 컴퓨터의 단축키 장부 번호. 실행할 때 그대로 돌려준다.
    Shortcuts {
        book: u64,
        items: Vec<ShortcutInfo>,
    },
}

/// 폰에 보이는 자료. 시계는 폰이 혼자 그리므로 없다.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum View {
    Performance,
    Processes,
    Weather,
    /// 큰 화면이면 세션과 GitHub/GitLab, 작은 칸이면 세션만
    Ai,
    Shortcuts,
}

impl From<View> for crate::View {
    fn from(view: View) -> Self {
        match view {
            View::Performance => Self::Performance,
            View::Processes => Self::Processes,
            View::Weather => Self::Weather,
            View::Ai => Self::Ai,
            View::Shortcuts => Self::Shortcuts,
        }
    }
}

/// 비율은 0~100, 네트워크는 초당 바이트.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PerformanceInfo {
    pub cpu: f32,
    pub gpu: Option<f32>,
    pub memory: f32,
    pub memory_used: u64,
    pub memory_total: u64,
    pub disk: f32,
    pub net_down: u64,
    pub net_up: u64,
}

impl From<Performance> for PerformanceInfo {
    fn from(p: Performance) -> Self {
        Self {
            cpu: p.cpu,
            gpu: p.gpu,
            memory: p.memory,
            memory_used: p.memory_used,
            memory_total: p.memory_total,
            disk: p.disk,
            net_down: p.net_down,
            net_up: p.net_up,
        }
    }
}

/// cpu 는 코어 하나가 100, memory 는 바이트, started_at 은 유닉스 밀리초.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub cpu: f32,
    pub memory: u64,
    pub started_at: u64,
}

impl From<Process> for ProcessInfo {
    fn from(p: Process) -> Self {
        Self {
            pid: p.pid,
            name: p.name,
            cpu: p.cpu,
            memory: p.memory,
            started_at: p.started_at,
        }
    }
}

/// 기기의 위치. 반올림은 core 가 한다.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct Place {
    pub latitude: f64,
    pub longitude: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum Sky {
    Unknown,
    Clear,
    PartlyCloudy,
    Cloudy,
    Fog,
    Drizzle,
    Rain,
    Snow,
    Shower,
    Thunderstorm,
}

impl From<crate::Sky> for Sky {
    fn from(sky: crate::Sky) -> Self {
        match sky {
            crate::Sky::Unknown => Self::Unknown,
            crate::Sky::Clear => Self::Clear,
            crate::Sky::PartlyCloudy => Self::PartlyCloudy,
            crate::Sky::Cloudy => Self::Cloudy,
            crate::Sky::Fog => Self::Fog,
            crate::Sky::Drizzle => Self::Drizzle,
            crate::Sky::Rain => Self::Rain,
            crate::Sky::Snow => Self::Snow,
            crate::Sky::Shower => Self::Shower,
            crate::Sky::Thunderstorm => Self::Thunderstorm,
        }
    }
}

/// 날씨를 실제로 받은 곳. Unknown 이면 출처를 숨긴다.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum WeatherSource {
    Unknown,
    Kma,
    OpenMeteo,
}

impl From<crate::Provider> for WeatherSource {
    fn from(provider: crate::Provider) -> Self {
        match provider {
            crate::Provider::Unknown => Self::Unknown,
            crate::Provider::Kma => Self::Kma,
            crate::Provider::OpenMeteo => Self::OpenMeteo,
        }
    }
}

/// 기온은 °C, fetched_at 은 받은 시각(유닉스 밀리초).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct WeatherInfo {
    pub temperature: f32,
    pub sky: Sky,
    pub high: f32,
    pub low: f32,
    pub is_day: bool,
    pub fetched_at: u64,
    pub source: WeatherSource,
}

impl From<crate::Weather> for WeatherInfo {
    fn from(w: crate::Weather) -> Self {
        Self {
            temperature: w.temperature,
            sky: crate::Sky::try_from(w.sky)
                .unwrap_or(crate::Sky::Unknown)
                .into(),
            high: w.high,
            low: w.low,
            is_day: w.is_day,
            fetched_at: w.fetched_at,
            source: crate::Provider::try_from(w.provider)
                .unwrap_or(crate::Provider::Unknown)
                .into(),
        }
    }
}

macro_rules! mirror {
    ($name:ident, $from:path, $($variant:ident),+) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            fn from_wire(value: i32) -> Self {
                use $from as Wire;
                match Wire::try_from(value) {
                    $(Ok(Wire::$variant) => Self::$variant,)+
                    Err(_) => Self::Unknown,
                }
            }
        }
    };
}

mirror!(AiTool, crate::AiTool, Unknown, ClaudeCode, Codex);
mirror!(AiState, crate::AiState, Unknown, Working, Waiting, Done);
mirror!(AiAlertKind, crate::AiAlertKind, Unknown, Done, Waiting);
mirror!(
    ForgeState,
    crate::ForgeState,
    Unknown,
    Ok,
    Missing,
    SignedOut,
    Failed
);

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ShortcutToggle {
    /// 상태 없는 버튼
    None,
    On,
    Off,
}

/// icon, color 는 이름표. system 이면 시스템 동작(그림만 그리는 칸).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ShortcutInfo {
    pub id: u32,
    pub name: String,
    pub icon: String,
    pub color: String,
    pub toggle: ShortcutToggle,
    pub system: bool,
}

impl From<crate::ShortcutItem> for ShortcutInfo {
    fn from(s: crate::ShortcutItem) -> Self {
        Self {
            id: s.id,
            name: s.name,
            icon: s.icon,
            color: s.color,
            toggle: match crate::ShortcutToggle::try_from(s.toggle) {
                Ok(crate::ShortcutToggle::On) => ShortcutToggle::On,
                Ok(crate::ShortcutToggle::Off) => ShortcutToggle::Off,
                _ => ShortcutToggle::None,
            },
            system: s.system,
        }
    }
}

/// since 는 지금 상태가 된 때(유닉스 밀리초)
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AiSessionInfo {
    pub id: String,
    pub tool: AiTool,
    pub title: String,
    pub project: String,
    pub state: AiState,
    pub since: u64,
    pub open: bool,
}

impl From<crate::AiSession> for AiSessionInfo {
    fn from(s: crate::AiSession) -> Self {
        Self {
            id: s.id,
            tool: AiTool::from_wire(s.tool),
            title: s.title,
            project: s.project,
            state: AiState::from_wire(s.state),
            since: s.since,
            open: s.open,
        }
    }
}

/// at 은 바뀜이 일어난 때(유닉스 밀리초)
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AiAlertInfo {
    pub id: u64,
    pub session_id: String,
    pub title: String,
    pub kind: AiAlertKind,
    pub at: u64,
}

impl From<crate::AiAlert> for AiAlertInfo {
    fn from(a: crate::AiAlert) -> Self {
        Self {
            id: a.id,
            session_id: a.session_id,
            title: a.title,
            kind: AiAlertKind::from_wire(a.kind),
            at: a.at,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ForgeInfo {
    pub state: ForgeState,
    pub reviews: u32,
    pub mine: u32,
    pub notices: u32,
}

impl From<crate::ForgeCounts> for ForgeInfo {
    fn from(c: crate::ForgeCounts) -> Self {
        Self {
            state: ForgeState::from_wire(c.state),
            reviews: c.reviews,
            mine: c.mine,
            notices: c.notices,
        }
    }
}

/// 이 접속에서 마지막으로 보낸 위치
#[derive(Clone, Copy, PartialEq, Eq)]
enum Sent {
    Never,
    Nowhere,
    At(i32, i32),
}

/// 명령 번호. 같은 명령을 다시 보낼 때는 같은 번호를 쓴다.
#[uniffi::export]
pub fn new_request_id() -> u64 {
    let mut bytes = [0; 8];
    getrandom::fill(&mut bytes).expect("난수를 얻지 못함");
    u64::from_le_bytes(bytes)
}

#[derive(Debug, PartialEq, thiserror::Error, uniffi::Error)]
pub enum LinkError {
    #[error("버전이 다름")]
    Version { mine: u8, theirs: u8 },
    #[error("등록되지 않은 컴퓨터")]
    NotPaired,
    #[error("등록을 취소함")]
    Rejected,
    #[error("{reason}")]
    Broken { reason: String },
}

impl From<SessionError> for LinkError {
    fn from(error: SessionError) -> Self {
        match error {
            SessionError::Version { mine, theirs } => Self::Version { mine, theirs },
            SessionError::NotPaired => Self::NotPaired,
            SessionError::Rejected => Self::Rejected,
            SessionError::Broken(reason) => Self::Broken {
                reason: reason.to_owned(),
            },
        }
    }
}

fn key(bytes: &[u8]) -> Result<[u8; KEY_LEN], LinkError> {
    bytes.try_into().map_err(|_| LinkError::Broken {
        reason: "키 길이가 틀림".to_owned(),
    })
}

/// 접속 하나를 맡는다. 새로 접속되면 새로 만든다.
#[derive(uniffi::Object)]
pub struct PhoneSession {
    session: Mutex<Session>,
    sent: Mutex<Sent>,
}

/// 블루투스 알림 신호의 서비스 데이터(PSM + 알림 신호 지문).
#[uniffi::export]
pub fn ble_service_data(psm: u16, public_key: Vec<u8>) -> Result<Vec<u8>, LinkError> {
    Ok(crate::service_data(psm, &key(&public_key)?))
}

#[uniffi::export]
pub fn ble_service_uuid() -> String {
    crate::BLE_SERVICE.to_owned()
}

#[uniffi::export]
impl PhoneSession {
    #[uniffi::constructor]
    pub fn new(
        private_key: Vec<u8>,
        public_key: Vec<u8>,
        computers: Vec<Vec<u8>>,
        allow_pairing: bool,
    ) -> Result<Arc<Self>, LinkError> {
        let identity = Identity {
            private: key(&private_key)?,
            public: key(&public_key)?,
        };
        let trusted = computers
            .iter()
            .map(|computer| key(computer))
            .collect::<Result<_, _>>()?;
        let session = Session::new(Role::Phone, &identity, trusted, false);
        let session = if allow_pairing {
            session
        } else {
            session.refusing_pairing()
        };
        Ok(Arc::new(Self {
            session: Mutex::new(session),
            sent: Mutex::new(Sent::Never),
        }))
    }

    pub fn feed(&self, bytes: Vec<u8>) -> Result<(), LinkError> {
        Ok(self.lock().feed(&bytes)?)
    }

    pub fn take_output(&self) -> Vec<u8> {
        self.lock().take_output()
    }

    pub fn next_event(&self) -> Option<PhoneEvent> {
        let sent = *lock(&self.sent);
        let mut session = self.lock();
        loop {
            return match session.next_event()? {
                Event::PairingCode(code) => Some(PhoneEvent::PairingCode { code }),
                Event::Paired { peer } => Some(PhoneEvent::Paired {
                    computer_key: peer.to_vec(),
                }),
                Event::Ready { peer } => Some(PhoneEvent::Ready {
                    computer_key: peer.to_vec(),
                }),
                Event::Message(Body::Hello(hello)) => Some(PhoneEvent::ComputerName {
                    name: hello.computer_name,
                }),
                Event::Message(Body::Performance(value)) => Some(PhoneEvent::Performance {
                    value: value.into(),
                }),
                Event::Message(Body::Processes(Processes { items })) => {
                    Some(PhoneEvent::Processes {
                        items: items.into_iter().map(Into::into).collect(),
                    })
                }
                Event::Message(Body::CommandResult(CommandResult {
                    request_id,
                    ok,
                    reason,
                })) => Some(PhoneEvent::CommandResult {
                    request_id,
                    ok,
                    reason,
                }),
                // 지금 보낸 위치의 값만 올린다.
                Event::Message(Body::Weather(weather))
                    if sent == Sent::At(weather.latitude_tenths, weather.longitude_tenths) =>
                {
                    Some(PhoneEvent::Weather {
                        value: weather.into(),
                    })
                }
                Event::Message(Body::AiSessions(sessions)) => Some(PhoneEvent::AiSessions {
                    items: sessions.items.into_iter().map(Into::into).collect(),
                }),
                Event::Message(Body::AiAlert(alert)) => Some(PhoneEvent::AiAlert {
                    value: alert.into(),
                }),
                Event::Message(Body::Shortcuts(shortcuts)) => Some(PhoneEvent::Shortcuts {
                    book: shortcuts.book,
                    items: shortcuts.items.into_iter().map(Into::into).collect(),
                }),
                Event::Message(Body::Forge(forge)) => Some(PhoneEvent::Forge {
                    github: forge.github.map(Into::into),
                    gitlab: forge.gitlab.map(Into::into),
                }),
                Event::Message(_) => continue,
            };
        }
    }

    pub fn confirm(&self, same: bool) -> Result<(), LinkError> {
        Ok(self.lock().confirm(same)?)
    }

    /// 지금 보이는 것 전체를 알린다. 바뀔 때마다, 다시 붙을 때마다 보낸다.
    /// 화면이 꺼지거나 앱이 뒤로 가면 `main` 없이 빈 `tiles` 로 보낸다.
    pub fn subscribe(&self, main: Option<View>, tiles: Vec<View>) -> Result<(), LinkError> {
        let main = main.map_or(crate::View::Nothing, Into::into) as i32;
        let tiles = tiles
            .into_iter()
            .map(|view| crate::View::from(view) as i32)
            .collect();
        Ok(self
            .lock()
            .send(Body::Subscribe(Subscribe { main, tiles }))?)
    }

    /// 위치를 알린다. null 은 위치 없음(권한 없음). 반올림은 여기서 한다.
    /// 이 접속에서 마지막으로 보낸 것과 같으면 보내지 않는다.
    pub fn send_location(&self, place: Option<Place>) -> Result<(), LinkError> {
        let next = match place.and_then(|p| place_tenths(p.latitude, p.longitude)) {
            Some((latitude, longitude)) => Sent::At(latitude, longitude),
            None => Sent::Nowhere,
        };
        let mut sent = lock(&self.sent);
        if *sent == next {
            return Ok(());
        }
        let location = match next {
            Sent::At(latitude, longitude) => Location {
                latitude_tenths: Some(latitude),
                longitude_tenths: Some(longitude),
            },
            _ => Location::default(),
        };
        self.lock().send(Body::Location(location))?;
        *sent = next;
        Ok(())
    }

    /// 단축키를 실행한다. book 은 그 목록을 받을 때 온 값. 결과는 `CommandResult` 로 온다.
    pub fn run_shortcut(
        &self,
        request_id: u64,
        shortcut_id: u32,
        book: u64,
    ) -> Result<(), LinkError> {
        Ok(self.lock().send(Body::RunShortcut(crate::RunShortcut {
            request_id,
            shortcut_id,
            book,
        }))?)
    }

    /// 사람이 한 번 더 확인한 뒤에 부른다. 결과는 `CommandResult` 로 온다.
    pub fn kill_process(
        &self,
        request_id: u64,
        pid: u32,
        started_at: u64,
    ) -> Result<(), LinkError> {
        Ok(self.lock().send(Body::KillProcess(KillProcess {
            request_id,
            pid,
            started_at,
        }))?)
    }
}

impl PhoneSession {
    fn lock(&self) -> MutexGuard<'_, Session> {
        lock(&self.session)
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|lock| lock.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Hello;

    #[test]
    fn a_known_computer_sends_its_name() {
        let computer = Identity::generate();
        let phone = generate_key_pair();
        let phone_key: [u8; KEY_LEN] = phone.public_key.clone().try_into().unwrap();
        let session = PhoneSession::new(
            phone.private_key,
            phone.public_key,
            vec![computer.public.to_vec()],
            true,
        )
        .unwrap();
        let mut c = Session::new(Role::Computer, &computer, vec![phone_key], false);
        for _ in 0..3 {
            session.feed(c.take_output()).unwrap();
            c.feed(&session.take_output()).unwrap();
        }
        c.send(Body::Hello(Hello {
            computer_name: "맥".into(),
        }))
        .unwrap();
        session.feed(c.take_output()).unwrap();
        assert_eq!(
            session.next_event(),
            Some(PhoneEvent::Ready {
                computer_key: computer.public.to_vec()
            })
        );
        assert_eq!(
            session.next_event(),
            Some(PhoneEvent::ComputerName { name: "맥".into() })
        );
    }

    #[test]
    fn subscribes_kills_and_reads_values() {
        let computer = Identity::generate();
        let phone = generate_key_pair();
        let phone_key: [u8; KEY_LEN] = phone.public_key.clone().try_into().unwrap();
        let session = PhoneSession::new(
            phone.private_key,
            phone.public_key,
            vec![computer.public.to_vec()],
            true,
        )
        .unwrap();
        let mut c = Session::new(Role::Computer, &computer, vec![phone_key], false);
        for _ in 0..3 {
            session.feed(c.take_output()).unwrap();
            c.feed(&session.take_output()).unwrap();
        }
        session
            .subscribe(Some(View::Processes), vec![View::Performance])
            .unwrap();
        session.kill_process(9, 42, 1_000).unwrap();
        c.feed(&session.take_output()).unwrap();
        let mut got = Vec::new();
        while let Some(event) = c.next_event() {
            got.push(event);
        }
        assert!(got.contains(&Event::Message(Body::Subscribe(Subscribe {
            main: crate::View::Processes as i32,
            tiles: vec![crate::View::Performance as i32],
        }))));
        assert!(got.contains(&Event::Message(Body::KillProcess(KillProcess {
            request_id: 9,
            pid: 42,
            started_at: 1_000,
        }))));
        c.send(Body::Performance(Performance {
            cpu: 12.5,
            gpu: None,
            ..Default::default()
        }))
        .unwrap();
        session.feed(c.take_output()).unwrap();
        assert!(matches!(
            session.next_event(),
            Some(PhoneEvent::Ready { .. })
        ));
        assert!(matches!(
            session.next_event(),
            Some(PhoneEvent::Performance { value }) if value.cpu == 12.5 && value.gpu.is_none()
        ));
        assert_ne!(new_request_id(), new_request_id());
    }

    fn ready() -> (Arc<PhoneSession>, Session) {
        let computer = Identity::generate();
        let phone = generate_key_pair();
        let phone_key: [u8; KEY_LEN] = phone.public_key.clone().try_into().unwrap();
        let session = PhoneSession::new(
            phone.private_key,
            phone.public_key,
            vec![computer.public.to_vec()],
            true,
        )
        .unwrap();
        let mut c = Session::new(Role::Computer, &computer, vec![phone_key], false);
        for _ in 0..3 {
            session.feed(c.take_output()).unwrap();
            c.feed(&session.take_output()).unwrap();
        }
        assert!(matches!(
            session.next_event(),
            Some(PhoneEvent::Ready { .. })
        ));
        (session, c)
    }

    fn locations(session: &PhoneSession, c: &mut Session) -> Vec<Location> {
        c.feed(&session.take_output()).unwrap();
        std::iter::from_fn(|| c.next_event())
            .filter_map(|event| match event {
                Event::Message(Body::Location(location)) => Some(location),
                _ => None,
            })
            .collect()
    }

    fn weather(latitude: i32, longitude: i32) -> Body {
        Body::Weather(crate::Weather {
            temperature: 3.5,
            latitude_tenths: latitude,
            longitude_tenths: longitude,
            provider: crate::Provider::Kma as i32,
            ..Default::default()
        })
    }

    #[test]
    fn location_is_rounded_once_and_not_repeated() {
        let (session, mut c) = ready();
        let seoul = Place {
            latitude: 37.5665,
            longitude: 126.978,
        };
        session.send_location(Some(seoul)).unwrap();
        session.send_location(Some(seoul)).unwrap();
        session
            .send_location(Some(Place {
                latitude: 37.6,
                longitude: 127.0,
            }))
            .unwrap();
        assert_eq!(
            locations(&session, &mut c),
            [Location {
                latitude_tenths: Some(376),
                longitude_tenths: Some(1270),
            }]
        );
        session
            .send_location(Some(Place {
                latitude: f64::NAN,
                longitude: 127.0,
            }))
            .unwrap();
        session.send_location(None).unwrap();
        assert_eq!(locations(&session, &mut c), [Location::default()]);
    }

    #[test]
    fn only_weather_for_the_sent_place_comes_up() {
        let (session, mut c) = ready();
        // 위치를 보내기 전 값은 버린다.
        c.send(weather(376, 1270)).unwrap();
        session.feed(c.take_output()).unwrap();
        assert_eq!(session.next_event(), None);
        session
            .send_location(Some(Place {
                latitude: 37.6,
                longitude: 127.0,
            }))
            .unwrap();
        c.send(weather(375, 1270)).unwrap();
        c.send(weather(376, 1270)).unwrap();
        session.feed(c.take_output()).unwrap();
        assert_eq!(
            session.next_event(),
            Some(PhoneEvent::Weather {
                value: WeatherInfo {
                    temperature: 3.5,
                    sky: Sky::Unknown,
                    high: 0.0,
                    low: 0.0,
                    is_day: false,
                    fetched_at: 0,
                    source: WeatherSource::Kma,
                }
            })
        );
        assert_eq!(session.next_event(), None);
        session.send_location(None).unwrap();
        c.send(weather(376, 1270)).unwrap();
        session.feed(c.take_output()).unwrap();
        assert_eq!(session.next_event(), None);
    }

    #[test]
    fn ai_messages_come_up() {
        let (session, mut c) = ready();
        session.subscribe(Some(View::Ai), vec![]).unwrap();
        c.feed(&session.take_output()).unwrap();
        assert!(std::iter::from_fn(|| c.next_event()).any(|e| e
            == Event::Message(Body::Subscribe(Subscribe {
                main: crate::View::Ai as i32,
                tiles: vec![],
            }))));
        c.send(Body::AiSessions(crate::AiSessions {
            items: vec![crate::AiSession {
                id: "abc".into(),
                tool: crate::AiTool::Codex as i32,
                title: "제목".into(),
                project: "폴더".into(),
                state: 42,
                since: 5,
                open: true,
            }],
        }))
        .unwrap();
        c.send(Body::AiAlert(crate::AiAlert {
            id: 1,
            session_id: "abc".into(),
            title: "제목".into(),
            kind: crate::AiAlertKind::Waiting as i32,
            at: 9,
        }))
        .unwrap();
        c.send(Body::Forge(crate::Forge {
            github: Some(crate::ForgeCounts {
                state: crate::ForgeState::Ok as i32,
                reviews: 1,
                mine: 2,
                notices: 3,
            }),
            gitlab: None,
        }))
        .unwrap();
        session.feed(c.take_output()).unwrap();
        let Some(PhoneEvent::AiSessions { items }) = session.next_event() else {
            panic!("세션 목록이 와야 함");
        };
        assert_eq!(items[0].tool, AiTool::Codex);
        // 모르는 상태 값은 Unknown
        assert_eq!(items[0].state, AiState::Unknown);
        assert!(matches!(
            session.next_event(),
            Some(PhoneEvent::AiAlert { value }) if value.kind == AiAlertKind::Waiting && value.at == 9
        ));
        assert_eq!(
            session.next_event(),
            Some(PhoneEvent::Forge {
                github: Some(ForgeInfo {
                    state: ForgeState::Ok,
                    reviews: 1,
                    mine: 2,
                    notices: 3
                }),
                gitlab: None,
            })
        );
    }

    #[test]
    fn shortcuts_come_up_and_runs_go_out() {
        let (session, mut c) = ready();
        session.run_shortcut(3, 7, 99).unwrap();
        c.feed(&session.take_output()).unwrap();
        assert!(std::iter::from_fn(|| c.next_event()).any(|e| e
            == Event::Message(Body::RunShortcut(crate::RunShortcut {
                request_id: 3,
                shortcut_id: 7,
                book: 99,
            }))));
        c.send(Body::Shortcuts(crate::Shortcuts {
            book: 99,
            items: vec![crate::ShortcutItem {
                id: 7,
                name: "터미널".into(),
                icon: "terminal".into(),
                color: "slate".into(),
                toggle: 42,
                system: false,
            }],
        }))
        .unwrap();
        session.feed(c.take_output()).unwrap();
        let Some(PhoneEvent::Shortcuts { book, items }) = session.next_event() else {
            panic!("단축키 목록이 와야 함");
        };
        assert_eq!(book, 99);
        assert_eq!(items[0].id, 7);
        assert_eq!(items[0].toggle, ShortcutToggle::None);
    }

    #[test]
    fn rejects_bad_keys() {
        assert!(PhoneSession::new(vec![0; 3], vec![0; 32], Vec::new(), true).is_err());
    }
}
