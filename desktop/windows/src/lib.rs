#![cfg(target_os = "windows")]

mod desk;
mod name;
mod secrets;
mod shortcut;

pub use desk::{WindowsMeter, WindowsProcesses};
pub use name::WindowsName;
pub use secrets::WindowsSecrets;
pub use shortcut::WindowsLauncher;
