mod wire;

#[cfg(feature = "uniffi")]
mod phone;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

pub use wire::{FrameReader, Hello, MAX_BODY, WireError, encode};
