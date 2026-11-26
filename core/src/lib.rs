mod proto;
mod session;
mod wire;

#[cfg(feature = "uniffi")]
mod phone;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

pub use proto::{Hello, envelope::Body};
pub use session::{Event, Identity, KEY_LEN, Role, Session, SessionError, VERSION};
pub use wire::{FrameReader, MAX_BODY, WireError, frame};
