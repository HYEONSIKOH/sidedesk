#![cfg(target_os = "windows")]

mod desk;
mod name;

pub use desk::{WindowsMeter, WindowsProcesses};
pub use name::WindowsName;
