//! 연결 위에서 주고받는 메시지. 번호는 바꾸지 않고 늘리기만 한다.

#[derive(Clone, PartialEq, prost::Message)]
pub struct Envelope {
    #[prost(
        oneof = "envelope::Body",
        tags = "1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13"
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
