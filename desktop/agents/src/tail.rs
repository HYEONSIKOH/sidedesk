//! 기록 파일을 늘어난 만큼만 읽는다. 처음 보는 파일은 끝부분만.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::time::SystemTime;

/// 처음 볼 때 읽는 끝부분
pub const FIRST: u64 = 256 * 1024;
/// 한 번에 이보다 많이 늘었으면 끝부분만 읽는다.
pub const LIMIT: u64 = 1024 * 1024;

#[derive(Default)]
pub struct Tail {
    offset: u64,
    /// 아직 줄 바꿈이 오지 않은 끝 조각
    carry: Vec<u8>,
    seen: bool,
    /// 같은 이름으로 새로 쓰였는지 보려는 파일 번호(유닉스)
    inode: u64,
    pub modified: Option<SystemTime>,
}

impl Tail {
    /// 새로 온 온전한 줄들. 바뀐 것이 없으면 빈 목록.
    /// 파일이 줄었으면(새로 쓰임) 처음 보는 것처럼 다시 읽는다.
    pub fn read(&mut self, path: &Path) -> std::io::Result<Vec<Vec<u8>>> {
        let meta = std::fs::metadata(path)?;
        let len = meta.len();
        self.modified = meta.modified().ok();
        if self.seen && len == self.offset {
            return Ok(Vec::new());
        }
        let inode = inode(&meta);
        if len < self.offset || (self.seen && inode != self.inode) {
            *self = Self {
                modified: self.modified,
                ..Self::default()
            };
        }
        let mut start = self.offset;
        let mut skip_partial = false;
        if !self.seen || len - self.offset > LIMIT {
            let first = if self.seen { LIMIT.min(FIRST) } else { FIRST };
            if len > first {
                start = len - first;
                skip_partial = true;
                self.carry.clear();
            }
        }
        let mut file = File::open(path)?;
        file.seek(SeekFrom::Start(start))?;
        let mut bytes = Vec::with_capacity((len - start) as usize);
        file.take(len - start).read_to_end(&mut bytes)?;
        self.offset = start + bytes.len() as u64;
        self.seen = true;
        self.inode = inode;
        let mut data = std::mem::take(&mut self.carry);
        data.extend_from_slice(&bytes);
        let mut lines: Vec<Vec<u8>> = data.split(|b| *b == b'\n').map(<[u8]>::to_vec).collect();
        // 마지막 조각은 줄 바꿈 전이라 다음에 잇는다.
        self.carry = lines.pop().unwrap_or_default();
        if skip_partial && !lines.is_empty() {
            lines.remove(0);
        }
        lines.retain(|line| !line.is_empty());
        Ok(lines)
    }
}

#[cfg(unix)]
fn inode(meta: &std::fs::Metadata) -> u64 {
    std::os::unix::fs::MetadataExt::ino(meta)
}

#[cfg(not(unix))]
fn inode(_meta: &std::fs::Metadata) -> u64 {
    0
}

/// 바이트 안에 글자가 있는지(줄을 해석하기 전에 거르는 데 쓴다)
pub fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp(name: &str) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("sidedesk-tail-{name}-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn reads_only_new_whole_lines() {
        let path = temp("lines");
        std::fs::write(&path, b"a\nb\nhal").unwrap();
        let mut tail = Tail::default();
        assert_eq!(tail.read(&path).unwrap(), [b"a".to_vec(), b"b".to_vec()]);
        assert!(tail.read(&path).unwrap().is_empty());
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all(b"f\nc\n").unwrap();
        assert_eq!(tail.read(&path).unwrap(), [b"half".to_vec(), b"c".to_vec()]);
        // 파일이 새로 쓰이면 처음부터
        std::fs::write(&path, b"x\n").unwrap();
        assert_eq!(tail.read(&path).unwrap(), [b"x".to_vec()]);
        // 같은 이름의 더 큰 새 파일로 바꿔치기돼도 처음부터
        let other = path.with_extension("new");
        std::fs::write(&other, b"one\ntwo\nthree\n").unwrap();
        std::fs::rename(&other, &path).unwrap();
        assert_eq!(
            tail.read(&path).unwrap(),
            [b"one".to_vec(), b"two".to_vec(), b"three".to_vec()]
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_big_file_is_read_from_its_end() {
        let path = temp("big");
        let mut data = Vec::new();
        for n in 0..40_000 {
            data.extend_from_slice(format!("line {n:08}\n").as_bytes());
        }
        std::fs::write(&path, &data).unwrap();
        let mut tail = Tail::default();
        let lines = tail.read(&path).unwrap();
        assert!(lines.len() < 40_000);
        assert_eq!(lines.last().unwrap(), b"line 00039999");
        // 잘린 앞 줄은 버린다.
        assert!(lines[0].starts_with(b"line "));
        assert_eq!(lines[0].len(), 13);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn finds_bytes() {
        assert!(contains(b"abc\"type\":\"ai-title\"", b"\"ai-title\""));
        assert!(!contains(b"abc", b"abcd"));
    }
}
