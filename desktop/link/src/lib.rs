#[cfg(all(feature = "dev-link", not(debug_assertions)))]
compile_error!("dev-link 는 암호화가 없어 배포용 빌드에 넣지 않는다");

#[cfg(feature = "dev-link")]
mod adb;
#[cfg(all(feature = "dev-link", target_os = "macos"))]
mod simulator;

#[cfg(feature = "dev-link")]
pub use adb::AdbLinks;
#[cfg(all(feature = "dev-link", target_os = "macos"))]
pub use simulator::SimulatorLinks;
