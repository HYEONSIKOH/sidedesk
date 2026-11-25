use std::io::{ErrorKind, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use sidedesk_domain::{Phone, PhoneLinks, PortError};

const SOCKET: &str = "/tmp/sidedesk-dev.sock";

/// iOS 시뮬레이터 앱이 만든 소켓 파일에 접속한다.
pub struct SimulatorLinks {
    socket: PathBuf,
}

impl Default for SimulatorLinks {
    fn default() -> Self {
        Self {
            socket: PathBuf::from(SOCKET),
        }
    }
}

impl PhoneLinks for SimulatorLinks {
    fn label(&self) -> &str {
        "iOS 시뮬레이터"
    }

    fn attached(&self) -> Result<Vec<Phone>, PortError> {
        Ok(if self.socket.exists() {
            vec![Phone {
                id: self.label().to_owned(),
                blocked: None,
            }]
        } else {
            Vec::new()
        })
    }

    fn send(&self, _id: &str, bytes: &[u8]) -> Result<(), PortError> {
        let failed = |error: std::io::Error| match error.kind() {
            ErrorKind::ConnectionRefused | ErrorKind::NotFound => {
                PortError::new("시뮬레이터의 앱이 꺼져 있거나 뒤에 있음")
            }
            _ => PortError::new(format!("시뮬레이터와 통신하지 못함: {error}")),
        };
        let mut stream = UnixStream::connect(&self.socket).map_err(failed)?;
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .map_err(failed)?;
        stream.write_all(bytes).map_err(failed)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Read;
    use std::os::unix::net::UnixListener;

    use super::*;

    fn links(name: &str) -> SimulatorLinks {
        let socket = std::env::temp_dir().join(format!("sd-{name}-{}.sock", std::process::id()));
        let _ = fs::remove_file(&socket);
        SimulatorLinks { socket }
    }

    #[test]
    fn no_socket_means_no_phone() {
        assert_eq!(links("none").attached(), Ok(Vec::new()));
    }

    #[test]
    fn sends_to_the_app() {
        let links = links("ok");
        let listener = UnixListener::bind(&links.socket).unwrap();
        assert_eq!(links.attached().unwrap().len(), 1);
        assert_eq!(links.send("", b"hi"), Ok(()));
        let mut got = Vec::new();
        listener.accept().unwrap().0.read_to_end(&mut got).unwrap();
        assert_eq!(got, b"hi");
        fs::remove_file(&links.socket).unwrap();
    }

    #[test]
    fn a_leftover_socket_means_the_app_is_off() {
        let links = links("stale");
        drop(UnixListener::bind(&links.socket).unwrap());
        assert!(links.send("", b"hi").unwrap_err().0.contains("꺼져"));
        fs::remove_file(&links.socket).unwrap();
    }
}
