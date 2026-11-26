mod beacon;
mod proto;
mod session;
mod wire;

#[cfg(feature = "uniffi")]
mod phone;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

pub use beacon::{
    BEACON_LEN, BLE_SERVICE, SERVICE_DATA_LEN, beacon, parse_service_data, service_data,
};
pub use proto::{
    CommandResult, Hello, KillProcess, Performance, Process, Processes, Subscribe, View,
    envelope::Body,
};
pub use session::{Event, Identity, KEY_LEN, Role, Session, SessionError, VERSION};
pub use wire::{FrameReader, MAX_BODY, WireError, frame};
