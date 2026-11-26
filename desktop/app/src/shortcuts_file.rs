//! 단축키 보관 파일(설정 폴더의 `shortcuts.json`).
//! 쓰기: 지금 파일을 `.bak` 으로 복사 → `.tmp` 에 쓰고 fsync → 바꿔치기.
//! 읽기: 깨졌으면 `.bak`, 둘 다 깨졌으면 빈 목록. 되돌리면 장부 번호를 새로 정해 옛 번호의 명령을 막는다.

use std::fs;
use std::hash::{BuildHasher, RandomState};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use sidedesk_domain::{PortError, Saved, ShortcutStore};

const FILE: &str = "shortcuts.json";

pub struct ShortcutsFile {
    path: PathBuf,
}

impl ShortcutsFile {
    pub fn new(folder: &Path) -> Self {
        Self {
            path: folder.join(FILE),
        }
    }

    fn with(&self, extension: &str) -> PathBuf {
        self.path.with_extension(format!("json.{extension}"))
    }
}

/// 컴퓨터마다 다른 장부 번호
fn new_book() -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    RandomState::new()
        .hash_one((nanos, std::process::id()))
        .max(1)
}

fn read(path: &Path) -> Option<Saved> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

impl ShortcutStore for ShortcutsFile {
    fn load(&self) -> Result<(Saved, Option<String>), PortError> {
        let exists = self.path.exists();
        if let Some(saved) = read(&self.path) {
            return Ok((saved, None));
        }
        let backup = self.with("bak");
        if !exists && !backup.exists() {
            // 처음 쓰는 컴퓨터
            return Ok((Saved::new(new_book()), None));
        }
        if exists {
            let _ = fs::rename(&self.path, self.with("broken"));
        }
        match read(&backup) {
            Some(mut saved) => {
                saved.book = new_book();
                Ok((
                    saved,
                    Some("단축키 설정이 깨져 직전 것으로 되돌렸습니다".to_owned()),
                ))
            }
            None => Ok((
                Saved::new(new_book()),
                Some("단축키 설정이 깨져 비웠습니다".to_owned()),
            )),
        }
    }

    fn save(&self, saved: &Saved) -> Result<(), PortError> {
        let failed = || PortError::new("파일에 쓰지 못했습니다");
        let text = serde_json::to_vec_pretty(saved).map_err(|_| failed())?;
        if self.path.exists() {
            fs::copy(&self.path, self.with("bak")).map_err(|_| failed())?;
        }
        let temporary = self.with("tmp");
        let _ = fs::remove_file(&temporary);
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut file = options.open(&temporary).map_err(|_| failed())?;
        file.write_all(&text)
            .and_then(|()| file.sync_all())
            .and_then(|()| fs::rename(&temporary, &self.path))
            .map_err(|_| failed())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sidedesk_domain::{Action, Shortcut, SystemAction};

    fn folder(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("sidedesk-shortcuts-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn sample(book: u64) -> Saved {
        let mut saved = Saved::new(book);
        saved.items.push(Shortcut {
            id: 1,
            name: "잠금".into(),
            icon: "lock".into(),
            color: "gray".into(),
            action: Action::System {
                action: SystemAction::Lock,
            },
        });
        saved.next_id = 2;
        saved
    }

    #[test]
    fn first_use_then_round_trip() {
        let dir = folder("trip");
        let file = ShortcutsFile::new(&dir);
        let (fresh, notice) = file.load().unwrap();
        assert!(notice.is_none() && fresh.items.is_empty() && fresh.book != 0);
        file.save(&sample(5)).unwrap();
        assert_eq!(file.load().unwrap(), (sample(5), None));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn broken_file_goes_back_to_the_backup_with_a_new_book() {
        let dir = folder("broken");
        let file = ShortcutsFile::new(&dir);
        file.save(&sample(5)).unwrap();
        file.save(&sample(5)).unwrap();
        fs::write(dir.join(FILE), b"{ broken").unwrap();
        let (saved, notice) = file.load().unwrap();
        assert_eq!(saved.items, sample(5).items);
        assert_ne!(saved.book, 5);
        assert!(notice.unwrap().contains("되돌렸습니다"));
        assert!(dir.join("shortcuts.json.broken").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn both_broken_gives_an_empty_list() {
        let dir = folder("both");
        let file = ShortcutsFile::new(&dir);
        fs::write(dir.join(FILE), b"x").unwrap();
        fs::write(dir.join("shortcuts.json.bak"), b"y").unwrap();
        let (saved, notice) = file.load().unwrap();
        assert!(saved.items.is_empty());
        assert!(notice.unwrap().contains("비웠습니다"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_file_with_a_backup_comes_back() {
        let dir = folder("missing");
        let file = ShortcutsFile::new(&dir);
        file.save(&sample(5)).unwrap();
        file.save(&sample(5)).unwrap();
        fs::remove_file(dir.join(FILE)).unwrap();
        let (saved, notice) = file.load().unwrap();
        assert_eq!(saved.items.len(), 1);
        assert!(notice.is_some());
        let _ = fs::remove_dir_all(&dir);
    }
}
