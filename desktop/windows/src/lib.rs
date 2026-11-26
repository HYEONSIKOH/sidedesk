#![cfg(target_os = "windows")]

mod desk;
mod music;
mod name;
mod secrets;
mod shortcut;

pub use desk::{WindowsMeter, WindowsProcesses};
pub use music::{WindowsPlayer, WindowsSpeaker};
pub use name::WindowsName;
pub use secrets::WindowsSecrets;
pub use shortcut::WindowsLauncher;
