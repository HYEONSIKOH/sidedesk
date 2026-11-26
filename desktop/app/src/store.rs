//! 키를 파일에 둔다. 맥은 ~/Library/Application Support/SideDesk, 윈도우는 %APPDATA%\SideDesk.

use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use sidedesk_core::{Identity, KEY_LEN};
use sidedesk_domain::{Keys, PortError};

const IDENTITY: &str = "identity.key";
const PHONES: &str = "phones.txt";
const LOCK: &str = "sidedesk.lock";

pub fn folder() -> Result<PathBuf, PortError> {
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(not(target_os = "windows"))]
    let base = std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join("Library/Application Support"));
    let folder = base
        .ok_or_else(|| PortError::new("설정 폴더를 찾지 못함"))?
        .join("SideDesk");
    fs::create_dir_all(&folder).map_err(|error| failed(&folder, error))?;
    Ok(folder)
}

fn failed(path: &Path, error: std::io::Error) -> PortError {
    PortError::new(format!("{}: {error}", path.display()))
}

/// `run` 과 `pair` 가 동시에 돌지 않게 한다. 돌려받은 파일을 쥐고 있는 동안 잠겨 있다.
pub fn lock(folder: &Path) -> Result<File, PortError> {
    let path = folder.join(LOCK);
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|error| failed(&path, error))?;
    file.try_lock()
        .map_err(|_| PortError::new("SideDesk 가 이미 실행 중임. 먼저 끄고 다시 실행"))?;
    Ok(file)
}

pub struct FileKeys {
    folder: PathBuf,
}

impl FileKeys {
    /// 시작할 때 한 번 만든다. 전에 쓰다 멈춘 임시 파일(쓰이지 않은 개인키)을 지운다.
    /// 이 폴더를 쓰는 프로세스는 잠금으로 하나뿐이라, 지금 쓰는 중인 파일은 없다.
    pub fn new(folder: PathBuf) -> Self {
        if let Ok(entries) = fs::read_dir(&folder) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with(IDENTITY) && name.ends_with(".tmp") {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
        Self { folder }
    }

    fn read_identity(&self, path: &Path) -> Result<Identity, PortError> {
        let bytes = fs::read(path).map_err(|error| failed(path, error))?;
        let broken = || PortError::new(format!("{}: 키 파일이 깨짐", path.display()));
        if bytes.len() != KEY_LEN * 2 {
            return Err(broken());
        }
        Ok(Identity {
            private: bytes[..KEY_LEN].try_into().map_err(|_| broken())?,
            public: bytes[KEY_LEN..].try_into().map_err(|_| broken())?,
        })
    }
}

impl Keys for FileKeys {
    fn identity(&self) -> Result<Identity, PortError> {
        let path = self.folder.join(IDENTITY);
        if path.exists() {
            return self.read_identity(&path);
        }
        // 임시 파일에 다 쓴 뒤 옮긴다. 쓰다 멈춰도 깨진 키 파일이 남지 않는다.
        let identity = Identity::generate();
        let temporary = self
            .folder
            .join(format!("{IDENTITY}.{:?}.tmp", std::thread::current().id()));
        let _ = fs::remove_file(&temporary);
        let written = private_file(&temporary, true)
            .and_then(|mut file| {
                file.write_all(&[identity.private, identity.public].concat())
                    .and_then(|()| file.sync_all())
            })
            .and_then(|()| fs::hard_link(&temporary, &path));
        let _ = fs::remove_file(&temporary);
        match written {
            Ok(()) => Ok(identity),
            // 다른 스레드나 프로세스가 먼저 만들었다. 덮어쓰지 않고 그것을 쓴다.
            Err(error) if error.kind() == ErrorKind::AlreadyExists => self.read_identity(&path),
            Err(error) => Err(failed(&path, error)),
        }
    }

    fn trusted(&self) -> Result<Vec<[u8; KEY_LEN]>, PortError> {
        let path = self.folder.join(PHONES);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(failed(&path, error)),
        };
        text.lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                decode(line.trim())
                    .ok_or_else(|| PortError::new(format!("{}: 깨진 줄이 있음", path.display())))
            })
            .collect()
    }

    fn trust(&self, key: [u8; KEY_LEN]) -> Result<(), PortError> {
        let mut keys = self.trusted()?;
        if keys.contains(&key) {
            return Ok(());
        }
        keys.push(key);
        let text: String = keys.iter().map(|key| encode(key) + "\n").collect();
        let path = self.folder.join(PHONES);
        let temporary = self.folder.join(format!("{PHONES}.tmp"));
        let _ = fs::remove_file(&temporary);
        let mut file = private_file(&temporary, true).map_err(|error| failed(&temporary, error))?;
        file.write_all(text.as_bytes())
            .and_then(|()| file.sync_all())
            .and_then(|()| fs::rename(&temporary, &path))
            .map_err(|error| failed(&path, error))
    }
}

/// 처음부터 나만 읽고 쓰는 파일로 만든다.
fn private_file(path: &Path, new: bool) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(new);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)
}

fn encode(key: &[u8]) -> String {
    key.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn decode(text: &str) -> Option<[u8; KEY_LEN]> {
    if text.len() != KEY_LEN * 2 || !text.is_ascii() {
        return None;
    }
    let bytes: Option<Vec<u8>> = (0..KEY_LEN)
        .map(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).ok())
        .collect();
    bytes?.try_into().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("sidedesk-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    #[test]
    fn keeps_the_same_identity() {
        let folder = temporary("identity");
        let keys = FileKeys::new(folder.clone());
        let first = keys.identity().unwrap();
        assert_eq!(keys.identity().unwrap().public, first.public);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(folder.join(IDENTITY))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn threads_starting_together_share_one_identity() {
        let folder = temporary("race");
        let shared = FileKeys::new(folder.clone());
        let keys: Vec<_> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|_| scope.spawn(|| shared.identity().unwrap().public))
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect()
        });
        assert!(keys.iter().all(|key| *key == keys[0]));
        assert_eq!(fs::read_dir(&folder).unwrap().count(), 1);
    }

    #[test]
    fn leftovers_from_a_crash_are_removed() {
        let folder = temporary("leftover");
        fs::write(
            folder.join(format!("{IDENTITY}.ThreadId(9).tmp")),
            b"secret",
        )
        .unwrap();
        FileKeys::new(folder.clone()).identity().unwrap();
        assert_eq!(fs::read_dir(&folder).unwrap().count(), 1);
    }

    #[test]
    fn a_broken_identity_is_reported_and_kept() {
        let folder = temporary("broken");
        fs::write(folder.join(IDENTITY), b"short").unwrap();
        assert!(
            FileKeys::new(folder.clone())
                .identity()
                .unwrap_err()
                .0
                .contains("깨짐")
        );
        assert_eq!(fs::read(folder.join(IDENTITY)).unwrap(), b"short");
    }

    #[test]
    fn trusts_each_phone_once() {
        let folder = temporary("trust");
        let keys = FileKeys::new(folder);
        assert!(keys.trusted().unwrap().is_empty());
        keys.trust([1; KEY_LEN]).unwrap();
        keys.trust([2; KEY_LEN]).unwrap();
        keys.trust([1; KEY_LEN]).unwrap();
        assert_eq!(keys.trusted().unwrap(), [[1; KEY_LEN], [2; KEY_LEN]]);
    }

    #[test]
    fn a_broken_phone_list_is_reported() {
        let folder = temporary("list");
        fs::write(folder.join(PHONES), "zz\n").unwrap();
        assert!(FileKeys::new(folder).trusted().is_err());
    }

    #[test]
    fn only_one_runs_at_a_time() {
        let folder = temporary("lock");
        let held = lock(&folder).unwrap();
        assert!(lock(&folder).is_err());
        drop(held);
        assert!(lock(&folder).is_ok());
    }
}
