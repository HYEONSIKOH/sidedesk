#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct PortError(pub String);

impl PortError {
    pub fn new(reason: impl Into<String>) -> Self {
        Self(reason.into())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Phone {
    pub id: String,
    /// 지금 보낼 수 없으면 그 이유
    pub blocked: Option<String>,
}

pub trait ComputerName {
    fn read(&self) -> Result<String, PortError>;
}

pub trait PhoneLinks {
    fn label(&self) -> &str;
    fn attached(&self) -> Result<Vec<Phone>, PortError>;
    fn send(&self, id: &str, bytes: &[u8]) -> Result<(), PortError>;
}
