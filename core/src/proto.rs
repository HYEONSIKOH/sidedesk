//! 연결 위에서 주고받는 메시지. 번호는 바꾸지 않고 늘리기만 한다.

#[derive(Clone, PartialEq, prost::Message)]
pub struct Envelope {
    #[prost(
        oneof = "envelope::Body",
        tags = "1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18"
    )]
    pub body: Option<envelope::Body>,
}

pub mod envelope {
    #[derive(Clone, PartialEq, prost::Oneof)]
    pub enum Body {
        #[prost(message, tag = "1")]
        Intro(super::Intro),
        #[prost(message, tag = "2")]
        PairCommit(super::Pairing),
        #[prost(message, tag = "3")]
        PairNonce(super::Pairing),
        #[prost(message, tag = "4")]
        PairReveal(super::Pairing),
        #[prost(message, tag = "5")]
        PairAnswer(super::Answer),
        #[prost(message, tag = "6")]
        Hello(super::Hello),
        #[prost(message, tag = "7")]
        Ping(super::Empty),
        #[prost(message, tag = "8")]
        Pong(super::Empty),
        /// 폰 → 컴퓨터
        #[prost(message, tag = "9")]
        Subscribe(super::Subscribe),
        /// 폰 → 컴퓨터
        #[prost(message, tag = "10")]
        KillProcess(super::KillProcess),
        /// 컴퓨터 → 폰
        #[prost(message, tag = "11")]
        Performance(super::Performance),
        /// 컴퓨터 → 폰
        #[prost(message, tag = "12")]
        Processes(super::Processes),
        /// 컴퓨터 → 폰
        #[prost(message, tag = "13")]
        CommandResult(super::CommandResult),
        /// 폰 → 컴퓨터
        #[prost(message, tag = "14")]
        Location(super::Location),
        /// 컴퓨터 → 폰
        #[prost(message, tag = "15")]
        Weather(super::Weather),
        /// 컴퓨터 → 폰
        #[prost(message, tag = "16")]
        AiSessions(super::AiSessions),
        /// 컴퓨터 → 폰
        #[prost(message, tag = "17")]
        AiAlert(super::AiAlert),
        /// 컴퓨터 → 폰
        #[prost(message, tag = "18")]
        Forge(super::Forge),
    }
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct Intro {
    #[prost(bool, tag = "1")]
    pub knows_you: bool,
    #[prost(bool, tag = "2")]
    pub wants_pairing: bool,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct Pairing {
    #[prost(bytes = "vec", tag = "1")]
    pub value: Vec<u8>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct Answer {
    #[prost(bool, tag = "1")]
    pub same: bool,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct Hello {
    #[prost(string, tag = "1")]
    pub computer_name: String,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct Empty {}

/// 폰에 보이는 자료. 시계처럼 폰이 혼자 그리는 칸은 넣지 않는다.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, prost::Enumeration)]
#[repr(i32)]
pub enum View {
    Nothing = 0,
    Performance = 1,
    Processes = 2,
    Weather = 3,
    /// main 이면 세션과 GitHub/GitLab, tiles 에만 있으면 세션만
    Ai = 4,
}

/// 지금 보이는 것 전체. 받을 때마다 앞의 구독을 바꾼다. 둘 다 비면 아무것도 안 본다.
#[derive(Clone, PartialEq, prost::Message)]
pub struct Subscribe {
    #[prost(enumeration = "View", tag = "1")]
    pub main: i32,
    #[prost(enumeration = "View", repeated, tag = "2")]
    pub tiles: Vec<i32>,
}

/// `request_id` 는 폰이 정하는 아무 숫자. 다시 보내도 한 번만 실행하고 같은 결과를 돌려준다.
#[derive(Clone, PartialEq, prost::Message)]
pub struct KillProcess {
    #[prost(uint64, tag = "1")]
    pub request_id: u64,
    #[prost(uint32, tag = "2")]
    pub pid: u32,
    /// 프로세스 시작 시각(유닉스 밀리초). 목록에서 받은 값 그대로.
    #[prost(uint64, tag = "3")]
    pub started_at: u64,
}

/// 비율은 0~100.
#[derive(Clone, PartialEq, prost::Message)]
pub struct Performance {
    #[prost(float, tag = "1")]
    pub cpu: f32,
    /// 읽을 수 없으면 없음
    #[prost(float, optional, tag = "2")]
    pub gpu: Option<f32>,
    #[prost(float, tag = "3")]
    pub memory: f32,
    #[prost(uint64, tag = "4")]
    pub memory_used: u64,
    #[prost(uint64, tag = "5")]
    pub memory_total: u64,
    /// 시동 디스크 공간 사용률
    #[prost(float, tag = "6")]
    pub disk: f32,
    /// 초당 바이트
    #[prost(uint64, tag = "7")]
    pub net_down: u64,
    #[prost(uint64, tag = "8")]
    pub net_up: u64,
}

/// CPU 상위와 메모리 상위를 합친 목록. 정렬은 폰이 한다.
#[derive(Clone, PartialEq, prost::Message)]
pub struct Processes {
    #[prost(message, repeated, tag = "1")]
    pub items: Vec<Process>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct Process {
    #[prost(uint32, tag = "1")]
    pub pid: u32,
    #[prost(string, tag = "2")]
    pub name: String,
    /// 코어 하나가 100. 여러 코어를 쓰면 100 을 넘는다.
    #[prost(float, tag = "3")]
    pub cpu: f32,
    /// 바이트
    #[prost(uint64, tag = "4")]
    pub memory: u64,
    /// 유닉스 밀리초
    #[prost(uint64, tag = "5")]
    pub started_at: u64,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct CommandResult {
    #[prost(uint64, tag = "1")]
    pub request_id: u64,
    #[prost(bool, tag = "2")]
    pub ok: bool,
    /// 실패한 까닭. 성공이면 비어 있다.
    #[prost(string, tag = "3")]
    pub reason: String,
}

/// 폰의 대략적인 위치(x10, 소수 첫째). 둘 다 없으면 위치 없음.
#[derive(Clone, PartialEq, prost::Message)]
pub struct Location {
    #[prost(sint32, optional, tag = "1")]
    pub latitude_tenths: Option<i32>,
    #[prost(sint32, optional, tag = "2")]
    pub longitude_tenths: Option<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, prost::Enumeration)]
#[repr(i32)]
pub enum Sky {
    Unknown = 0,
    Clear = 1,
    PartlyCloudy = 2,
    Cloudy = 3,
    Fog = 4,
    Drizzle = 5,
    Rain = 6,
    Snow = 7,
    Shower = 8,
    Thunderstorm = 9,
}

/// 실제로 날씨를 받은 곳
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, prost::Enumeration)]
#[repr(i32)]
pub enum Provider {
    Unknown = 0,
    Kma = 1,
    OpenMeteo = 2,
}

/// 기온은 °C, 최고와 최저는 오늘 값.
#[derive(Clone, PartialEq, prost::Message)]
pub struct Weather {
    #[prost(float, tag = "1")]
    pub temperature: f32,
    #[prost(float, tag = "2")]
    pub high: f32,
    #[prost(float, tag = "3")]
    pub low: f32,
    #[prost(enumeration = "Sky", tag = "4")]
    pub sky: i32,
    #[prost(bool, tag = "5")]
    pub is_day: bool,
    /// 받은 시각(유닉스 밀리초)
    #[prost(uint64, tag = "6")]
    pub fetched_at: u64,
    /// 어느 위치의 값인지(x10)
    #[prost(sint32, tag = "7")]
    pub latitude_tenths: i32,
    #[prost(sint32, tag = "8")]
    pub longitude_tenths: i32,
    #[prost(enumeration = "Provider", tag = "9")]
    pub provider: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, prost::Enumeration)]
#[repr(i32)]
pub enum AiTool {
    Unknown = 0,
    ClaudeCode = 1,
    Codex = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, prost::Enumeration)]
#[repr(i32)]
pub enum AiState {
    Unknown = 0,
    Working = 1,
    /// 답변 대기(도구 허락이나 사람의 답)
    Waiting = 2,
    Done = 3,
}

/// AI 세션 하나. 대화 내용은 담지 않는다.
#[derive(Clone, PartialEq, prost::Message)]
pub struct AiSession {
    /// 한 실행 안에서만 같은 짧은 해시
    #[prost(string, tag = "1")]
    pub id: String,
    #[prost(enumeration = "AiTool", tag = "2")]
    pub tool: i32,
    #[prost(string, tag = "3")]
    pub title: String,
    /// 작업 폴더의 마지막 이름
    #[prost(string, tag = "4")]
    pub project: String,
    #[prost(enumeration = "AiState", tag = "5")]
    pub state: i32,
    /// 지금 상태가 된 때(유닉스 밀리초)
    #[prost(uint64, tag = "6")]
    pub since: u64,
    /// 프로그램이 아직 떠 있는지
    #[prost(bool, tag = "7")]
    pub open: bool,
}

/// 작업 중과 답변 대기를 먼저, 그다음 최근에 끝난 것. 바뀔 때마다 통째로.
#[derive(Clone, PartialEq, prost::Message)]
pub struct AiSessions {
    #[prost(message, repeated, tag = "1")]
    pub items: Vec<AiSession>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, prost::Enumeration)]
#[repr(i32)]
pub enum AiAlertKind {
    Unknown = 0,
    Done = 1,
    Waiting = 2,
}

/// 작업 중에서 끝남이나 답변 대기로 바뀐 때 한 번
#[derive(Clone, PartialEq, prost::Message)]
pub struct AiAlert {
    /// 컴퓨터 프로그램이 켜진 뒤 1부터 늘어난다.
    #[prost(uint64, tag = "1")]
    pub id: u64,
    #[prost(string, tag = "2")]
    pub session_id: String,
    #[prost(string, tag = "3")]
    pub title: String,
    #[prost(enumeration = "AiAlertKind", tag = "4")]
    pub kind: i32,
    /// 바뀜이 일어난 때(유닉스 밀리초)
    #[prost(uint64, tag = "5")]
    pub at: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, prost::Enumeration)]
#[repr(i32)]
pub enum ForgeState {
    Unknown = 0,
    Ok = 1,
    /// 프로그램(gh, glab)이 없음
    Missing = 2,
    SignedOut = 3,
    Failed = 4,
}

#[derive(Clone, Copy, PartialEq, prost::Message)]
pub struct ForgeCounts {
    #[prost(enumeration = "ForgeState", tag = "1")]
    pub state: i32,
    /// 나에게 온 검토 요청
    #[prost(uint32, tag = "2")]
    pub reviews: u32,
    /// 내가 연 요청
    #[prost(uint32, tag = "3")]
    pub mine: u32,
    /// 읽지 않은 알림
    #[prost(uint32, tag = "4")]
    pub notices: u32,
}

#[derive(Clone, Copy, PartialEq, prost::Message)]
pub struct Forge {
    #[prost(message, optional, tag = "1")]
    pub github: Option<ForgeCounts>,
    #[prost(message, optional, tag = "2")]
    pub gitlab: Option<ForgeCounts>,
}
