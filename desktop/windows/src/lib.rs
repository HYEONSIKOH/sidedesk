#![cfg(target_os = "windows")]

mod handle;
mod http;
mod meter;
mod name;
mod process;
mod secrets;
mod shortcut;

pub use http::WinGet;
pub use meter::WindowsMeter;
pub use name::WindowsName;
pub use process::WindowsProcesses;
pub use secrets::WindowsSecrets;
pub use shortcut::WindowsLauncher;
