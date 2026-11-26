//! 연결 위에서 주고받는 메시지. 번호는 바꾸지 않고 늘리기만 한다.

#[derive(Clone, PartialEq, prost::Message)]
pub struct Envelope {
    #[prost(oneof = "envelope::Body", tags = "1, 2, 3, 4, 5, 6, 7, 8")]
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
