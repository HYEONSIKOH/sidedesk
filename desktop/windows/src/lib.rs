#![cfg(target_os = "windows")]

mod com;
mod handle;
mod http;
mod image;
mod meter;
mod music;
mod name;
mod process;
mod secrets;
mod shortcut;

pub use http::WinGet;
pub use meter::WindowsMeter;
pub use music::{WindowsPlayer, WindowsSpeaker};
pub use name::WindowsName;
pub use process::WindowsProcesses;
pub use secrets::WindowsSecrets;
pub use shortcut::WindowsLauncher;
