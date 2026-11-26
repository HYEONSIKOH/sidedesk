mod beacon;
mod ios;
mod place;
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
pub use ios::{BLE_INFO, IOS_USB_PORT};
pub use place::{place_tenths, valid_tenths};
pub use proto::{
    CommandResult, Hello, KillProcess, Location, Performance, Process, Processes, Provider, Sky,
    Subscribe, View, Weather, envelope::Body,
};
pub use session::{Event, Identity, KEY_LEN, Role, Session, SessionError, VERSION};
pub use wire::{FrameReader, MAX_BODY, WireError, frame};
