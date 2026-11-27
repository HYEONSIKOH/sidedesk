//! 맥 시스템 "지금 재생 중": 제어 센터에 나오는 곡을 앱과 상관없이 읽는다(브라우저 영상 포함).
//! macOS 15.4 부터 MediaRemote 는 애플 프로그램만 읽을 수 있어, 애플의 /usr/bin/perl 이 함께 넣은
//! mediaremote-adapter(BSD-3-Clause) 프레임워크를 올려 읽는다. 프레임워크가 없으면 쓰지 않는다(`find`).
//! 곡 제목은 로그나 오류 문장에 남기지 않는다.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;
use sidedesk_core::{MusicAction, MusicPlayer};
use sidedesk_domain::{Changed, Picture, Player, PortError, Track};

use crate::image;
use crate::runner::Runner;

const PERL: &str = "/usr/bin/perl";
const SCRIPT: &str = "mediaremote-adapter.pl";
const FRAMEWORK: &str = "MediaRemoteAdapter.framework";
const LIMIT: Duration = Duration::from_secs(4);
/// 그림이 든 출력(base64)을 받을 크기
const ART_OUTPUT: usize = 8 * 1024 * 1024;
const MUSIC_ID: &str = "com.apple.Music";
const SPOTIFY_ID: &str = "com.spotify.client";
// MRCommand 번호
const TOGGLE: &str = "2";
const NEXT: &str = "4";
const PREVIOUS: &str = "5";

pub struct NowPlaying {
    script: PathBuf,
    framework: PathBuf,
    runner: Runner,
    art_runner: Runner,
    /// 그림을 마지막으로 읽은 곡
    art_key: Mutex<Option<String>>,
    stream: Mutex<Option<Child>>,
}

impl NowPlaying {
    /// `dir` 에 스크립트와 프레임워크가 있고 읽을 수 있으면 쓴다.
    pub fn find(dir: &Path, home: PathBuf) -> Option<Self> {
        let script = dir.join(SCRIPT);
        let framework = dir.join(FRAMEWORK);
        if !Path::new(PERL).exists() || !script.is_file() || !framework.is_dir() {
            return None;
        }
        let player = Self {
            runner: Runner::new(PathBuf::from(PERL), home.clone(), LIMIT),
            art_runner: Runner::new(PathBuf::from(PERL), home, LIMIT).keeping(ART_OUTPUT),
            script,
            framework,
            art_key: Mutex::default(),
            stream: Mutex::default(),
        };
        // 권한 검사가 바뀌어 못 읽게 되면 기존 방식으로 돌아간다.
        matches!(player.run(&["test"]), Some((true, _))).then_some(player)
    }

    /// 앱 묶음 안의 자리(Contents/Resources/mediaremote)
    pub fn bundled() -> Option<PathBuf> {
        let exe = std::env::current_exe().ok()?;
        Some(exe.parent()?.parent()?.join("Resources/mediaremote"))
    }

    fn base(&self) -> Vec<String> {
        vec![
            self.script.to_string_lossy().into_owned(),
            self.framework.to_string_lossy().into_owned(),
        ]
    }

    fn run(&self, args: &[&str]) -> Option<(bool, Vec<u8>)> {
        self.run_with(&self.runner, args)
    }

    fn run_with(&self, runner: &Runner, args: &[&str]) -> Option<(bool, Vec<u8>)> {
        let base = self.base();
        let mut all: Vec<&str> = base.iter().map(String::as_str).collect();
        all.extend_from_slice(args);
        runner.run(&all, true)
    }

    fn art_key(&self) -> MutexGuard<'_, Option<String>> {
        self.art_key.lock().unwrap_or_else(|l| l.into_inner())
    }

    fn stream(&self) -> MutexGuard<'_, Option<Child>> {
        self.stream.lock().unwrap_or_else(|l| l.into_inner())
    }

    fn artwork(&self) -> Option<Picture> {
        let (ok, out) = self.run_with(&self.art_runner, &["get"])?;
        if !ok {
            return None;
        }
        let value: Value = serde_json::from_slice(&out).ok()?;
        let data = value.get("artworkData")?.as_str()?;
        image::shrink(&decode_base64(data)?)
    }
}

/// `get --micros --no-artwork` 출력을 곡으로. 재생 중인 것이 없으면(null) 없음.
fn parse(out: &[u8], now: SystemTime) -> Option<Track> {
    let value: Value = serde_json::from_slice(out).ok()?;
    let object = value.as_object()?;
    let text = |key: &str| -> String {
        object
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect()
    };
    let micros = |key: &str| object.get(key).and_then(Value::as_f64).unwrap_or(0.0).max(0.0);
    let title = text("title");
    if title.is_empty() {
        return None;
    }
    let bundle = text("parentApplicationBundleIdentifier");
    let bundle = if bundle.is_empty() { text("bundleIdentifier") } else { bundle };
    let player = match bundle.as_str() {
        MUSIC_ID => MusicPlayer::Music,
        SPOTIFY_ID => MusicPlayer::Spotify,
        _ => MusicPlayer::System,
    };
    let playing = object.get("playing").and_then(Value::as_bool).unwrap_or(false);
    // 위치는 timestamp 순간의 값이다. 재생 중이면 지금까지 흐른 만큼 더한다.
    let mut position = micros("elapsedTimeMicros");
    let stamp = micros("timestampEpochMicros");
    if playing && stamp > 0.0 {
        let now_micros = now.duration_since(UNIX_EPOCH).map(|d| d.as_micros() as f64).unwrap_or(0.0);
        position += (now_micros - stamp).max(0.0);
    }
    let duration = micros("durationMicros");
    if duration > 0.0 {
        position = position.min(duration);
    }
    let enabled = |id: u64| {
        object
            .get("supportedCommands")
            .and_then(Value::as_array)
            .map(|all| {
                all.iter().any(|c| {
                    c.get("id").and_then(Value::as_u64) == Some(id)
                        && c.get("enabled").and_then(Value::as_bool).unwrap_or(true)
                })
            })
            .unwrap_or(true)
    };
    let id = object
        .get("contentItemIdentifier")
        .or_else(|| object.get("uniqueIdentifier"))
        .map(|v| v.to_string())
        .unwrap_or_default();
    Some(Track {
        player,
        key: format!("{bundle}\u{1f}{id}\u{1f}{title}\u{1f}{}", text("artist")),
        title,
        artist: text("artist"),
        album: text("album"),
        duration_ms: (duration / 1000.0) as u64,
        playing,
        position_ms: (position / 1000.0) as u64,
        measured_at: now,
        can_play: enabled(2),
        can_next: enabled(4),
        can_previous: enabled(5),
    })
}

/// 표준 base64 (줄바꿈 무시). 깨졌으면 없음.
fn decode_base64(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let mut buffer = 0u32;
    let mut bits = 0;
    for byte in text.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' | b'\n' | b'\r' => continue,
            _ => return None,
        };
        buffer = (buffer << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    Some(out)
}

impl Player for NowPlaying {
    fn now(&self) -> Result<(Track, Option<Picture>), PortError> {
        let Some((true, out)) = self.run(&["get", "--micros", "--no-artwork"]) else {
            return Err(PortError::new("재생 정보를 읽지 못했습니다"));
        };
        let Some(track) = parse(&out, SystemTime::now()) else {
            *self.art_key() = None;
            return Ok((Track::nothing(), None));
        };
        let new_song = {
            let mut key = self.art_key();
            let new = key.as_deref() != Some(track.key.as_str());
            *key = Some(track.key.clone());
            new
        };
        let picture = new_song.then(|| self.artwork()).flatten();
        Ok((track, picture))
    }

    fn command(&self, action: MusicAction) -> Result<(), PortError> {
        let id = match action {
            MusicAction::PlayPause => TOGGLE,
            MusicAction::Next => NEXT,
            MusicAction::Previous => PREVIOUS,
            _ => return Err(PortError::new("지원하지 않는 조작입니다")),
        };
        match self.run(&["send", id]) {
            Some((true, _)) => Ok(()),
            _ => Err(PortError::new("실행하지 못했습니다")),
        }
    }

    /// 보는 동안 `stream` 을 띄워 두고, 한 줄이 올 때마다 다시 읽게 알린다.
    fn watch(&self, changed: Option<Changed>) {
        let mut stream = self.stream();
        let Some(changed) = changed else {
            if let Some(mut child) = stream.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
            return;
        };
        if stream.is_some() || self.runner.stopped() {
            return;
        }
        let child = Command::new(PERL)
            .args(self.base())
            .args(["stream", "--micros", "--no-artwork", "--debounce=150"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let Ok(mut child) = child else {
            return;
        };
        if let Some(out) = child.stdout.take() {
            let changed = Arc::clone(&changed);
            thread::spawn(move || {
                for line in BufReader::new(out).lines() {
                    if line.is_err() {
                        break;
                    }
                    changed();
                }
            });
        }
        *stream = Some(child);
    }

    fn stop(&self) {
        self.watch(None);
        self.runner.stop();
        self.art_runner.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AT: u64 = 1_795_761_900_000_000;

    fn at(micros: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_micros(micros)
    }

    #[test]
    fn reads_a_playing_song_and_moves_the_position_to_now() {
        let out = r#"{"title":"곡","artist":"가수","album":"앨범","bundleIdentifier":"com.apple.Music",
            "contentItemIdentifier":"1::2","playing":true,"durationMicros":200000000,
            "elapsedTimeMicros":10000000,"timestampEpochMicros":1795761895000000,
            "supportedCommands":[{"id":2,"enabled":true},{"id":4,"enabled":false},{"id":5}]}"#;
        let track = parse(out.as_bytes(), at(AT)).unwrap();
        assert_eq!(track.player, MusicPlayer::Music);
        assert_eq!((track.title.as_str(), track.artist.as_str()), ("곡", "가수"));
        assert_eq!(track.duration_ms, 200_000);
        assert_eq!(track.position_ms, 15_000, "5초 흐름");
        assert!(track.playing && track.can_play && !track.can_next && track.can_previous);
    }

    #[test]
    fn paused_songs_keep_their_position_and_browsers_are_system() {
        let out = r#"{"title":"영상","bundleIdentifier":"com.google.Chrome.helper",
            "parentApplicationBundleIdentifier":"com.google.Chrome","playing":false,
            "elapsedTimeMicros":3000000,"timestampEpochMicros":1795761800000000}"#;
        let track = parse(out.as_bytes(), at(AT)).unwrap();
        assert_eq!(track.player, MusicPlayer::System);
        assert_eq!(track.position_ms, 3_000);
        assert_eq!(track.duration_ms, 0, "길이 모름");
    }

    #[test]
    fn nothing_playing_or_broken_output_is_none() {
        assert!(parse(b"null", at(AT)).is_none());
        assert!(parse(b"", at(AT)).is_none());
        assert!(parse(br#"{"playing":true}"#, at(AT)).is_none(), "제목 없음");
        assert!(parse(b"{broken", at(AT)).is_none());
    }

    #[test]
    fn position_never_passes_the_end() {
        let out = r#"{"title":"곡","playing":true,"durationMicros":1000000,
            "elapsedTimeMicros":900000,"timestampEpochMicros":1795761800000000}"#;
        assert_eq!(parse(out.as_bytes(), at(AT)).unwrap().position_ms, 1_000);
    }

    #[test]
    fn base64_decodes_and_rejects_junk() {
        assert_eq!(decode_base64("aGVs\nbG8=").unwrap(), b"hello");
        assert_eq!(decode_base64("").unwrap(), b"");
        assert!(decode_base64("a*b").is_none());
    }

    #[test]
    fn missing_adapter_is_not_used() {
        assert!(NowPlaying::find(Path::new("/nonexistent"), std::env::temp_dir()).is_none());
    }
}
