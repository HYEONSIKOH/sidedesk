//! 키를 파일에 둔다. 맥은 ~/Library/Application Support/SideDesk, 윈도우는 %APPDATA%\SideDesk.

use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use sidedesk_core::{Identity, KEY_LEN};
use sidedesk_domain::{Keys, Paired, PortError};

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
        .map_err(|_| PortError::new("SideDesk 가 이미 실행 중입니다. 메뉴 막대나 알림 영역의 SideDesk 아이콘에서 설정 창을 여세요"))?;
    Ok(file)
}

pub struct FileKeys {
    folder: PathBuf,
    /// 짝 목록을 읽고 고쳐 쓰는 동안 다른 스레드가 끼어들지 못하게 한다.
    phones: Mutex<()>,
}

impl FileKeys {
    /// 시작할 때 한 번 만든다. 전에 쓰다 멈춘 임시 파일(쓰이지 않은 개인키)을 지운다.
    /// 이 폴더를 쓰는 프로세스는 잠금으로 하나뿐이라, 지금 쓰는 중인 파일은 없다.
    pub fn new(folder: PathBuf) -> Self {
        if let Ok(entries) = fs::read_dir(&folder) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if (name.starts_with(IDENTITY) || name.starts_with(PHONES))
                    && name.ends_with(".tmp")
                {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
        Self {
            folder,
            phones: Mutex::new(()),
        }
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

    fn trusted(&self) -> Result<Vec<Paired>, PortError> {
        let _held = self.phones.lock().unwrap_or_else(|lock| lock.into_inner());
        self.read_phones()
    }

    fn trust(&self, key: [u8; KEY_LEN], name: &str, serial: &str) -> Result<(), PortError> {
        let _held = self.phones.lock().unwrap_or_else(|lock| lock.into_inner());
        let mut phones = self.read_phones()?;
        let fresh = Paired {
            key,
            name: name.to_owned(),
            serial: serial.to_owned(),
        };
        match phones.iter_mut().find(|paired| paired.key == key) {
            Some(saved) => *saved = fresh,
            None => phones.push(fresh),
        }
        self.write_phones(&phones)
    }

    fn update(&self, key: [u8; KEY_LEN], name: &str, serial: &str) -> Result<(), PortError> {
        let _held = self.phones.lock().unwrap_or_else(|lock| lock.into_inner());
        let mut phones = self.read_phones()?;
        let Some(saved) = phones.iter_mut().find(|paired| paired.key == key) else {
            return Ok(());
        };
        saved.name = name.to_owned();
        saved.serial = serial.to_owned();
        self.write_phones(&phones)
    }

    fn forget(&self, key: [u8; KEY_LEN]) -> Result<(), PortError> {
        let _held = self.phones.lock().unwrap_or_else(|lock| lock.into_inner());
        let mut phones = self.read_phones()?;
        let before = phones.len();
        phones.retain(|paired| paired.key != key);
        if phones.len() == before {
            return Ok(());
        }
        self.write_phones(&phones)
    }
}

impl FileKeys {
    /// 한 줄에 `키16진수<TAB>이름<TAB>일련번호`. 이름이나 일련번호가 없는 옛 줄도 읽는다.
    fn read_phones(&self) -> Result<Vec<Paired>, PortError> {
        let path = self.folder.join(PHONES);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(failed(&path, error)),
        };
        text.lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                let mut fields = line.split('\t');
                let key = decode(fields.next().unwrap_or("").trim())
                    .ok_or_else(|| PortError::new(format!("{}: 깨진 줄이 있음", path.display())))?;
                Ok(Paired {
                    key,
                    name: fields.next().unwrap_or("").trim().to_owned(),
                    serial: fields.next().unwrap_or("").trim().to_owned(),
                })
            })
            .collect()
    }

    fn write_phones(&self, phones: &[Paired]) -> Result<(), PortError> {
        let text: String = phones
            .iter()
            .map(|paired| {
                format!(
                    "{}\t{}\t{}\n",
                    encode(&paired.key),
                    paired.name.replace(['\t', '\n'], " "),
                    paired.serial.replace(['\t', '\n'], " ")
                )
            })
            .collect();
        let path = self.folder.join(PHONES);
        let temporary = self
            .folder
            .join(format!("{PHONES}.{}.tmp", std::process::id()));
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
    fn trusts_each_phone_once_and_keeps_the_newest_name() {
        let folder = temporary("trust");
        let keys = FileKeys::new(folder);
        assert!(keys.trusted().unwrap().is_empty());
        keys.trust([1; KEY_LEN], "A", "s").unwrap();
        keys.trust([2; KEY_LEN], "B", "s").unwrap();
        keys.trust([1; KEY_LEN], "A2", "s").unwrap();
        let names: Vec<_> = keys
            .trusted()
            .unwrap()
            .into_iter()
            .map(|p| (p.key[0], p.name))
            .collect();
        assert_eq!(names, [(1, "A2".to_owned()), (2, "B".to_owned())]);
    }

    #[test]
    fn update_does_not_bring_back_a_forgotten_phone() {
        let folder = temporary("update");
        let keys = FileKeys::new(folder);
        keys.trust([1; KEY_LEN], "", "").unwrap();
        keys.update([1; KEY_LEN], "SM A716S", "R3C").unwrap();
        keys.forget([1; KEY_LEN]).unwrap();
        keys.update([1; KEY_LEN], "SM A716S", "R3C").unwrap();
        assert!(keys.trusted().unwrap().is_empty());
    }

    #[test]
    fn reads_old_lines_without_a_name() {
        let folder = temporary("old");
        fs::write(folder.join(PHONES), format!("{}\n", encode(&[3; KEY_LEN]))).unwrap();
        assert_eq!(
            FileKeys::new(folder).trusted().unwrap(),
            [Paired {
                key: [3; KEY_LEN],
                name: String::new(),
                serial: String::new()
            }]
        );
    }

    #[test]
    fn forgets_a_phone() {
        let folder = temporary("forget");
        let keys = FileKeys::new(folder);
        keys.trust([1; KEY_LEN], "A", "s").unwrap();
        keys.trust([2; KEY_LEN], "B", "s").unwrap();
        keys.forget([1; KEY_LEN]).unwrap();
        keys.forget([9; KEY_LEN]).unwrap();
        assert_eq!(
            keys.trusted().unwrap(),
            [Paired {
                key: [2; KEY_LEN],
                name: "B".into(),
                serial: "s".into()
            }]
        );
    }

    #[test]
    fn forgetting_and_trusting_together_lose_nothing() {
        let folder = temporary("together");
        let keys = FileKeys::new(folder);
        for i in 0..20u8 {
            keys.trust([i; KEY_LEN], "x", "s").unwrap();
        }
        std::thread::scope(|scope| {
            for i in 0..20u8 {
                let keys = &keys;
                scope.spawn(move || keys.forget([i; KEY_LEN]).unwrap());
                scope.spawn(move || keys.trust([100 + i; KEY_LEN], "y", "s").unwrap());
            }
        });
        let left: Vec<_> = keys
            .trusted()
            .unwrap()
            .into_iter()
            .map(|p| p.key[0])
            .collect();
        assert_eq!(left.len(), 20);
        assert!(left.iter().all(|first| *first >= 100));
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
