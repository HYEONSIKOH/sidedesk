//! 맥 음악: 음악 앱과 Spotify 를 AppleScript(osascript)로 읽고 조작한다.
//! 스크립트는 바뀌지 않는 글이고 사용자 글을 넣지 않는다(`format!` 금지). 그림 파일 경로만 인자로 넘긴다.
//! 꺼진 앱에는 묻지 않는다: 먼저 떠 있는지 보고, 스크립트 안에서도 `is running` 으로 한 번 더 막는다.

use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use core_foundation::base::{CFType, TCFType};
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::string::{CFString, CFStringRef};
use core_foundation_sys::notification_center::{
    CFNotificationCenterAddObserver, CFNotificationCenterGetDistributedCenter,
    CFNotificationCenterRef, CFNotificationCenterRemoveEveryObserver,
    CFNotificationSuspensionBehaviorDeliverImmediately,
};
use objc2::rc::autoreleasepool;
use objc2_app_kit::NSRunningApplication;
use objc2_foundation::NSString;
use sidedesk_core::{MusicAction, MusicPlayer};
use sidedesk_domain::{Changed, Level, Picture, Player, PortError, Speaker, Track};

use crate::runner::Runner;
use crate::{audio, image};

const OSASCRIPT: &str = "/usr/bin/osascript";
const LIMIT: Duration = Duration::from_secs(5);
const SEP: char = '\u{1f}';
const MUSIC_ID: &str = "com.apple.Music";
const SPOTIFY_ID: &str = "com.spotify.client";
const MUSIC_NOTICE: &str = "com.apple.Music.playerInfo";
const SPOTIFY_NOTICE: &str = "com.spotify.client.PlaybackStateChanged";

const MUSIC_NOW: &str = r#"on run argv
	set sep to character id 31
	if application id "com.apple.Music" is not running then return "NONE"
	try
		tell application id "com.apple.Music"
			set st to (player state as string)
			if st is "stopped" then return "STOPPED"
			set t to current track
			set k to persistent ID of t
			set n to name of t
			set a to artist of t
			set al to album of t
			set d to ((duration of t) * 1000) as integer
			set p to ((player position) * 1000) as integer
		end tell
		return "OK" & sep & st & sep & k & sep & n & sep & a & sep & al & sep & d & sep & p
	on error number n
		return "ERR" & sep & n
	end try
end run"#;

const MUSIC_ART: &str = r#"on run argv
	if application id "com.apple.Music" is not running then return "NONE"
	try
		tell application id "com.apple.Music"
			set d to raw data of artwork 1 of current track
		end tell
		set f to open for access (POSIX file (item 1 of argv)) with write permission
		set eof f to 0
		write d to f
		close access f
		return "OK"
	on error number n
		try
			close access (POSIX file (item 1 of argv))
		end try
		return "ERR" & (character id 31) & n
	end try
end run"#;

const SPOTIFY_NOW: &str = r#"on run argv
	set sep to character id 31
	if application id "com.spotify.client" is not running then return "NONE"
	try
		tell application id "com.spotify.client"
			set st to (player state as string)
			if st is "stopped" then return "STOPPED"
			set t to current track
			set k to id of t
			set n to name of t
			set a to artist of t
			set al to album of t
			set d to duration of t
			set p to ((player position) * 1000) as integer
		end tell
		return "OK" & sep & st & sep & k & sep & n & sep & a & sep & al & sep & d & sep & p
	on error number n
		return "ERR" & sep & n
	end try
end run"#;

const MUSIC_PLAYPAUSE: &str = r#"if application id "com.apple.Music" is running then tell application id "com.apple.Music" to playpause"#;
const MUSIC_NEXT: &str = r#"if application id "com.apple.Music" is running then tell application id "com.apple.Music" to next track"#;
const MUSIC_PREVIOUS: &str = r#"if application id "com.apple.Music" is running then tell application id "com.apple.Music" to previous track"#;
const SPOTIFY_PLAYPAUSE: &str = r#"if application id "com.spotify.client" is running then tell application id "com.spotify.client" to playpause"#;
const SPOTIFY_NEXT: &str = r#"if application id "com.spotify.client" is running then tell application id "com.spotify.client" to next track"#;
const SPOTIFY_PREVIOUS: &str = r#"if application id "com.spotify.client" is running then tell application id "com.spotify.client" to previous track"#;

/// osascript 한 줄 읽기의 결과
#[derive(Debug, PartialEq)]
enum Read {
    /// 앱이 꺼져 있음
    None,
    Stopped,
    /// 권한 거절(-1743), 자동 재생 곡(-1728) 같은 오류 번호
    Error(i64),
    Playing(Track),
}

/// `OK␟상태␟열쇠␟제목␟아티스트␟앨범␟길이ms␟위치ms` 를 읽는다. 칸 수가 다르면 읽기 실패.
fn parse(output: &str, player: MusicPlayer, measured_at: SystemTime) -> Option<Read> {
    let line = output.trim_end_matches(['\n', '\r']);
    let fields: Vec<&str> = line.split(SEP).collect();
    match fields.as_slice() {
        ["NONE"] => Some(Read::None),
        ["STOPPED"] => Some(Read::Stopped),
        ["ERR", number] => number.trim().parse().ok().map(Read::Error),
        ["OK", state, key, title, artist, album, duration, position] => {
            let clean = |text: &str| -> String {
                text.chars()
                    .map(|c| if c.is_control() { ' ' } else { c })
                    .collect()
            };
            Some(Read::Playing(Track {
                player,
                key: (*key).to_owned(),
                title: clean(title),
                artist: clean(artist),
                album: clean(album),
                duration_ms: duration.trim().parse().unwrap_or(0),
                playing: *state == "playing",
                position_ms: position.trim().parse().unwrap_or(0),
                measured_at,
                can_play: true,
                can_next: true,
                can_previous: true,
            }))
        }
        _ => None,
    }
}

fn running(bundle: &str) -> bool {
    autoreleasepool(|_| {
        NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str(bundle))
            .iter()
            .any(|app| !app.isTerminated())
    })
}

#[derive(Default)]
struct Memory {
    /// 마지막으로 고른 플레이어(둘 다 멈춤이면 이것)
    last: Option<MusicPlayer>,
    /// 그림을 읽은 곡의 열쇠
    art_key: Option<(MusicPlayer, String)>,
    /// Spotify 가 "Stopped" 를 알리면 잠깐 묻지 않는다(끄는 중인 앱을 깨우지 않게).
    spotify_quiet_until: Option<Instant>,
}

/// 분산 알림을 받을 함수. 알림은 메인 스레드 런루프로 온다(2026-11-27 실험: 다른 스레드 런루프로는 오지 않음).
/// 앱(창)은 메인 스레드가 런루프를 돌려 받고, 창 없는 `run` 모드는 받지 못해 주기적으로 다시 읽기만 한다.
/// 관찰자를 뗀 뒤에도 메인 스레드에서 도는 중일 수 있어 이 값은 MacPlayer 가 끝까지 들고 있다.
struct Listener {
    changed: Mutex<Option<Changed>>,
    memory: Arc<Mutex<Memory>>,
    attached: Mutex<bool>,
}

pub struct MacPlayer {
    runner: Runner,
    art_file: PathBuf,
    memory: Arc<Mutex<Memory>>,
    listener: Arc<Listener>,
    names: Vec<&'static str>,
}

impl MacPlayer {
    /// `cache` 는 SideDesk 가 쓰는 캐시 폴더(그림을 잠깐 내려놓는 곳)
    pub fn new(home: PathBuf, cache: PathBuf) -> Self {
        Self::with_names(home, cache, vec![MUSIC_NOTICE, SPOTIFY_NOTICE])
    }

    fn with_names(home: PathBuf, cache: PathBuf, names: Vec<&'static str>) -> Self {
        let memory: Arc<Mutex<Memory>> = Arc::default();
        Self {
            runner: Runner::new(PathBuf::from(OSASCRIPT), home, LIMIT),
            art_file: cache.join("artwork.bin"),
            listener: Arc::new(Listener {
                changed: Mutex::new(None),
                memory: Arc::clone(&memory),
                attached: Mutex::new(false),
            }),
            memory,
            names,
        }
    }

    fn memory(&self) -> std::sync::MutexGuard<'_, Memory> {
        self.memory.lock().unwrap_or_else(|l| l.into_inner())
    }

    fn script(&self, script: &str, args: &[&str]) -> Option<String> {
        let mut all = vec!["-e", script];
        all.extend_from_slice(args);
        let (ok, out) = self.runner.run(&all, true)?;
        ok.then(|| String::from_utf8_lossy(&out).into_owned())
    }

    fn read(&self, player: MusicPlayer) -> Read {
        let (bundle, script) = match player {
            MusicPlayer::Music => (MUSIC_ID, MUSIC_NOW),
            MusicPlayer::Spotify => (SPOTIFY_ID, SPOTIFY_NOW),
            _ => return Read::None,
        };
        if !running(bundle) {
            return Read::None;
        }
        if player == MusicPlayer::Spotify
            && self
                .memory()
                .spotify_quiet_until
                .is_some_and(|until| Instant::now() < until)
        {
            return Read::None;
        }
        self.script(script, &[])
            .and_then(|out| parse(&out, player, SystemTime::now()))
            .unwrap_or(Read::Error(0))
    }

    /// 음악 앱 그림(Spotify 는 인터넷 주소뿐이라 받지 않음, 의문점 28)
    fn artwork(&self) -> Option<Picture> {
        let path = self.art_file.to_str()?;
        if let Some(parent) = self.art_file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let out = self.script(MUSIC_ART, &[path])?;
        let bytes = (out.trim() == "OK")
            .then(|| std::fs::read(&self.art_file).ok())
            .flatten();
        let _ = std::fs::remove_file(&self.art_file);
        bytes.and_then(|b| image::shrink(&b))
    }
}

impl Player for MacPlayer {
    fn now(&self) -> Result<(Track, Option<Picture>), PortError> {
        let reads = [MusicPlayer::Music, MusicPlayer::Spotify].map(|p| (p, self.read(p)));
        if reads.iter().any(|(_, r)| matches!(r, Read::Error(-1743)))
            && !reads.iter().any(|(_, r)| matches!(r, Read::Playing(_)))
        {
            return Err(PortError::new(
                "음악 앱 제어를 허용해 주세요. 설정 창의 안내를 보세요",
            ));
        }
        let last = self.memory().last;
        let playing: Vec<Track> = reads
            .into_iter()
            .filter_map(|(_, r)| match r {
                Read::Playing(track) => Some(track),
                _ => None,
            })
            .collect();
        // 재생 중인 쪽, 둘 다 멈춤이면 마지막에 고른 쪽
        let chosen = playing
            .iter()
            .find(|t| t.playing)
            .or_else(|| playing.iter().find(|t| Some(t.player) == last))
            .or_else(|| playing.first())
            .cloned();
        let Some(track) = chosen else {
            self.memory().art_key = None;
            return Ok((Track::nothing(), None));
        };
        let key = (track.player, track.key.clone());
        let new_song = {
            let mut memory = self.memory();
            memory.last = Some(track.player);
            let new = memory.art_key.as_ref() != Some(&key);
            memory.art_key = Some(key);
            new
        };
        let picture = (new_song && track.player == MusicPlayer::Music)
            .then(|| self.artwork())
            .flatten();
        Ok((track, picture))
    }

    fn command(&self, action: MusicAction) -> Result<(), PortError> {
        let player = self.memory().last.unwrap_or(MusicPlayer::None);
        let script = match (player, action) {
            (MusicPlayer::Music, MusicAction::PlayPause) => MUSIC_PLAYPAUSE,
            (MusicPlayer::Music, MusicAction::Next) => MUSIC_NEXT,
            (MusicPlayer::Music, MusicAction::Previous) => MUSIC_PREVIOUS,
            (MusicPlayer::Spotify, MusicAction::PlayPause) => SPOTIFY_PLAYPAUSE,
            (MusicPlayer::Spotify, MusicAction::Next) => SPOTIFY_NEXT,
            (MusicPlayer::Spotify, MusicAction::Previous) => SPOTIFY_PREVIOUS,
            _ => return Err(PortError::new("재생 중인 곡이 없습니다")),
        };
        match self.runner.run(&["-e", script], false) {
            Some((true, _)) => Ok(()),
            _ => Err(PortError::new("실행하지 못했습니다")),
        }
    }

    fn watch(&self, changed: Option<Changed>) {
        let listener = &self.listener;
        let mut attached = listener.attached.lock().unwrap_or_else(|l| l.into_inner());
        let watching = changed.is_some();
        *listener.changed.lock().unwrap_or_else(|l| l.into_inner()) = changed;
        if watching == *attached {
            return;
        }
        // SAFETY: 분산 알림 센터
        let center = unsafe { CFNotificationCenterGetDistributedCenter() };
        let observer = Arc::as_ptr(listener).cast::<c_void>();
        if watching {
            for name in &self.names {
                let name = CFString::new(name);
                // SAFETY: observer 는 MacPlayer 가 끝까지 쥐고 있는 Listener 를 가리킨다.
                unsafe {
                    CFNotificationCenterAddObserver(
                        center,
                        observer,
                        notified,
                        name.as_concrete_TypeRef(),
                        std::ptr::null(),
                        CFNotificationSuspensionBehaviorDeliverImmediately,
                    );
                }
            }
        } else {
            // SAFETY: 위에서 붙인 관찰자를 모두 뗀다.
            unsafe { CFNotificationCenterRemoveEveryObserver(center, observer) };
        }
        *attached = watching;
    }

    fn stop(&self) {
        self.watch(None);
        self.runner.stop();
    }
}

extern "C" fn notified(
    _center: CFNotificationCenterRef,
    observer: *mut c_void,
    name: CFStringRef,
    _object: *const c_void,
    info: CFDictionaryRef,
) {
    if observer.is_null() {
        return;
    }
    // SAFETY: observer 는 listen 이 쥐고 있는 Listener 다. 알림은 관찰자를 떼기 전에만 온다.
    let listener = unsafe { &*observer.cast::<Listener>() };
    let stopped = !name.is_null() && !info.is_null() && {
        // SAFETY: 센터가 넘긴 값은 이 호출 동안 살아 있다.
        let name = unsafe { CFString::wrap_under_get_rule(name) }.to_string();
        let info: CFDictionary<CFString, CFType> =
            unsafe { CFDictionary::wrap_under_get_rule(info) };
        name == SPOTIFY_NOTICE
            && info
                .find(CFString::from_static_string("Player State"))
                .and_then(|v| v.downcast::<CFString>())
                .is_some_and(|s| s == "Stopped")
    };
    if stopped {
        listener
            .memory
            .lock()
            .unwrap_or_else(|l| l.into_inner())
            .spotify_quiet_until = Some(Instant::now() + Duration::from_secs(3));
    }
    let changed = listener
        .changed
        .lock()
        .unwrap_or_else(|l| l.into_inner())
        .clone();
    if let Some(changed) = changed {
        changed();
    }
}

/// 기본 출력 장치의 볼륨
pub struct MacSpeaker;

impl Speaker for MacSpeaker {
    fn read(&self) -> Option<Level> {
        audio::volume()
    }

    fn set(&self, level: u32) -> Result<(), PortError> {
        audio::set_volume(level)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn line(fields: &[&str]) -> String {
        fields.join(&SEP.to_string())
    }

    #[test]
    fn reads_script_lines() {
        let at = SystemTime::now();
        let ok = line(&[
            "OK",
            "playing",
            "AB12",
            "곡\t이름",
            "부른 이",
            "앨범",
            "215000",
            "61000",
        ]);
        let Some(Read::Playing(track)) = parse(&ok, MusicPlayer::Music, at) else {
            panic!("곡을 읽어야 함");
        };
        assert_eq!(track.title, "곡 이름");
        assert_eq!((track.duration_ms, track.position_ms), (215_000, 61_000));
        assert!(track.playing);
        assert_eq!(parse("NONE\n", MusicPlayer::Music, at), Some(Read::None));
        assert_eq!(
            parse("STOPPED", MusicPlayer::Music, at),
            Some(Read::Stopped)
        );
        assert_eq!(
            parse(&line(&["ERR", "-1743"]), MusicPlayer::Music, at),
            Some(Read::Error(-1743))
        );
        // 칸이 모자라면 읽기 실패
        assert_eq!(
            parse(&line(&["OK", "playing", "x"]), MusicPlayer::Music, at),
            None
        );
        let paused = line(&["OK", "paused", "k", "t", "a", "al", "0", "0"]);
        assert!(matches!(
            parse(&paused, MusicPlayer::Spotify, at),
            Some(Read::Playing(t)) if !t.playing && t.player == MusicPlayer::Spotify
        ));
    }

    #[test]
    fn scripts_never_take_user_text() {
        for script in [
            MUSIC_NOW,
            MUSIC_ART,
            SPOTIFY_NOW,
            MUSIC_PLAYPAUSE,
            SPOTIFY_NEXT,
        ] {
            assert!(script.contains("is running") || script.contains("is not running"));
            assert!(!script.contains("{}"));
        }
    }

    #[test]
    fn watching_attaches_once_and_notices_call_back() {
        const TEST: &str = "io.github.hyeonsikoh.sidedesk.test-notice";
        let player = MacPlayer::with_names(std::env::temp_dir(), std::env::temp_dir(), vec![TEST]);
        let hits = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&hits);
        player.watch(Some(Arc::new(move || {
            count.fetch_add(1, Ordering::SeqCst);
        })));
        player.watch(Some(Arc::new({
            let count = Arc::clone(&hits);
            move || {
                count.fetch_add(1, Ordering::SeqCst);
            }
        })));
        assert!(*player.listener.attached.lock().unwrap());
        // 알림은 메인 런루프로 와서 시험 하네스에서는 오지 않는다. 받는 함수를 직접 불러 본다.
        let observer = Arc::as_ptr(&player.listener) as *mut c_void;
        let spotify = CFString::new(SPOTIFY_NOTICE);
        let info: CFDictionary<CFString, CFType> = CFDictionary::from_CFType_pairs(&[(
            CFString::from_static_string("Player State"),
            CFString::from_static_string("Stopped").as_CFType(),
        )]);
        notified(
            std::ptr::null_mut(),
            observer,
            spotify.as_concrete_TypeRef(),
            std::ptr::null(),
            info.as_concrete_TypeRef(),
        );
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        assert!(player.memory().spotify_quiet_until.is_some());
        player.watch(None);
        player.watch(None);
        assert!(!*player.listener.attached.lock().unwrap());
        notified(
            std::ptr::null_mut(),
            observer,
            spotify.as_concrete_TypeRef(),
            std::ptr::null(),
            std::ptr::null(),
        );
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[test]
    #[ignore = "이 맥의 음악 앱, Spotify 가 떠 있는지와 재생 상태만 찍는다(제목은 찍지 않음)"]
    fn reads_players_without_titles() {
        let player = MacPlayer::new(std::env::temp_dir(), std::env::temp_dir());
        eprintln!(
            "음악 앱 떠 있음 {}, Spotify 떠 있음 {}",
            running(MUSIC_ID),
            running(SPOTIFY_ID)
        );
        match player.now() {
            Ok((track, picture)) => eprintln!(
                "플레이어 {:?}, 재생 {}, 그림 {}",
                track.player,
                track.playing,
                picture.is_some()
            ),
            Err(error) => eprintln!("오류: {}", error.0),
        }
    }
}
