//! 윈도우 음악. 시스템 미디어(시스템 미디어 컨트롤에 붙는 모든 플레이어)의 지금 세션을 읽고 조작한다.
//! 볼륨은 기본 출력 장치.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sidedesk_core::{MusicAction, MusicPlayer};
use sidedesk_domain::{Changed, Level, Picture, Player, PortError, Speaker, Track};
use windows::Foundation::TypedEventHandler;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Storage::Streams::{DataReader, IRandomAccessStreamReference};

use crate::com::speaker;
use crate::image::{SOURCE_LIMIT, shrink};

/// 세션 관리자를 못 열었으면 이만큼 뒤에 다시 연다.
const RETRY: Duration = Duration::from_secs(60);
/// 1601-01-01 부터 1970-01-01 까지(100ns 단위)
const UNIX_EPOCH_TICKS: i64 = 116_444_736_000_000_000;

/// 지금 등록해 둔 바뀜 알림
struct Watching {
    changed: Changed,
    manager_token: i64,
    session: Option<(Session, [i64; 3])>,
}

#[derive(Default)]
struct State {
    manager: Option<Manager>,
    failed_at: Option<Instant>,
    /// 그림을 마지막으로 보낸 곡
    art_key: Option<String>,
    watching: Option<Watching>,
}

/// 시스템 미디어 재생 정보
#[derive(Default)]
pub struct WindowsPlayer {
    state: Arc<Mutex<State>>,
}

fn lock(state: &Mutex<State>) -> std::sync::MutexGuard<'_, State> {
    state.lock().unwrap_or_else(|lock| lock.into_inner())
}

impl WindowsPlayer {
    fn manager(&self) -> Option<Manager> {
        let mut state = lock(&self.state);
        if let Some(manager) = &state.manager {
            return Some(manager.clone());
        }
        if state.failed_at.is_some_and(|at| at.elapsed() < RETRY) {
            return None;
        }
        match Manager::RequestAsync().and_then(|request| request.join()) {
            Ok(manager) => {
                state.manager = Some(manager.clone());
                state.failed_at = None;
                Some(manager)
            }
            Err(_) => {
                state.failed_at = Some(Instant::now());
                None
            }
        }
    }

    fn session(&self) -> Option<Session> {
        self.manager()?.GetCurrentSession().ok()
    }
}

/// WinRT 시각(1601 부터 100ns)을 SystemTime 으로. 0 이나 미래 값은 없음.
fn system_time(ticks: i64) -> Option<SystemTime> {
    let unix = ticks.checked_sub(UNIX_EPOCH_TICKS)?;
    let at = UNIX_EPOCH + Duration::from_nanos(u64::try_from(unix).ok()?.checked_mul(100)?);
    (ticks > 0 && at <= SystemTime::now()).then_some(at)
}

fn millis(ticks: i64) -> u64 {
    u64::try_from(ticks).unwrap_or(0) / 10_000
}

/// 지금 세션에서 곡 하나를 읽는다.
fn read(session: &Session) -> windows::core::Result<(Track, Option<IRandomAccessStreamReference>)> {
    let properties = session.TryGetMediaPropertiesAsync()?.join()?;
    let playback = session.GetPlaybackInfo()?;
    let controls = playback.Controls()?;
    let timeline = session.GetTimelineProperties()?;
    let app = session
        .SourceAppUserModelId()
        .unwrap_or_default()
        .to_string();
    let title = properties.Title()?.to_string();
    let artist = properties.Artist().unwrap_or_default().to_string();
    let album = properties.AlbumTitle().unwrap_or_default().to_string();
    let start = timeline.StartTime()?.Duration;
    let end = timeline.EndTime()?.Duration;
    let duration_ms = millis(end - start);
    let mut position_ms = millis(timeline.Position()?.Duration - start);
    if duration_ms > 0 {
        position_ms = position_ms.min(duration_ms);
    }
    // 플레이어가 위치를 마지막으로 알린 때. 0 이나 미래 값이면 지금으로 본다.
    let measured_at = timeline
        .LastUpdatedTime()
        .ok()
        .and_then(|at| system_time(at.UniversalTime))
        .unwrap_or_else(SystemTime::now);
    let playing = playback.PlaybackStatus()? == Status::Playing;
    let track = Track {
        player: MusicPlayer::System,
        key: format!("{app}\n{title}\n{artist}\n{album}"),
        title,
        artist,
        album,
        duration_ms,
        playing,
        position_ms,
        measured_at,
        can_play: controls.IsPlayPauseToggleEnabled()?
            || controls.IsPlayEnabled()?
            || controls.IsPauseEnabled()?,
        can_next: controls.IsNextEnabled()?,
        can_previous: controls.IsPreviousEnabled()?,
    };
    Ok((track, properties.Thumbnail().ok()))
}

/// 썸네일 바이트. 너무 크면 읽지 않는다.
fn thumbnail(reference: &IRandomAccessStreamReference) -> Option<Vec<u8>> {
    let stream = reference.OpenReadAsync().ok()?.join().ok()?;
    let size = stream.Size().ok()?;
    if size == 0 || size > SOURCE_LIMIT as u64 {
        return None;
    }
    let reader = DataReader::CreateDataReader(&stream).ok()?;
    let loaded = reader.LoadAsync(size as u32).ok()?.join().ok()? as usize;
    let mut bytes = vec![0u8; loaded];
    reader.ReadBytes(&mut bytes).ok()?;
    Some(bytes)
}

impl Player for WindowsPlayer {
    fn now(&self) -> Result<(Track, Option<Picture>), PortError> {
        let Some(session) = self.session() else {
            lock(&self.state).art_key = None;
            return Ok((Track::nothing(), None));
        };
        let (track, reference) =
            read(&session).map_err(|_| PortError::new("재생 정보를 읽지 못했습니다"))?;
        if track.title.is_empty() {
            lock(&self.state).art_key = None;
            return Ok((Track::nothing(), None));
        }
        let new_song = {
            let mut state = lock(&self.state);
            let new = state.art_key.as_deref() != Some(track.key.as_str());
            state.art_key = Some(track.key.clone());
            new
        };
        let picture = if new_song {
            reference
                .as_ref()
                .and_then(thumbnail)
                .and_then(|bytes| shrink(&bytes))
        } else {
            None
        };
        // 세션이 바뀌었으면 알림도 새 세션으로 옮긴다.
        self.follow(&session);
        Ok((track, picture))
    }

    fn command(&self, action: MusicAction) -> Result<(), PortError> {
        let session = self
            .session()
            .ok_or_else(|| PortError::new("재생 중인 곡이 없습니다"))?;
        let asked = match action {
            MusicAction::PlayPause => session.TryTogglePlayPauseAsync(),
            MusicAction::Next => session.TrySkipNextAsync(),
            MusicAction::Previous => session.TrySkipPreviousAsync(),
            _ => return Err(PortError::new("재생 중인 곡이 없습니다")),
        };
        match asked.and_then(|request| request.join()) {
            Ok(true) => Ok(()),
            Ok(false) => Err(PortError::new("플레이어가 받지 않았습니다")),
            Err(_) => Err(PortError::new("실행하지 못했습니다")),
        }
    }

    fn watch(&self, changed: Option<Changed>) {
        self.unwatch();
        let (Some(changed), Some(manager)) = (changed, self.manager()) else {
            return;
        };
        // 처리기는 알림 함수만 잡는다(플레이어를 잡지 않아 순환 참조가 없다). 새 세션의 알림은 다음 now() 에서 건다.
        let ping = changed.clone();
        let Ok(manager_token) =
            manager.CurrentSessionChanged(&TypedEventHandler::new(move |_, _| {
                ping();
                Ok(())
            }))
        else {
            return;
        };
        lock(&self.state).watching = Some(Watching {
            changed,
            manager_token,
            session: None,
        });
        if let Some(session) = self.session() {
            self.follow(&session);
        }
    }
}

impl WindowsPlayer {
    /// 지금 세션의 바뀜 알림을 건다. 이미 같은 세션이면 그대로.
    fn follow(&self, session: &Session) {
        let mut state = lock(&self.state);
        let Some(watching) = state.watching.as_mut() else {
            return;
        };
        if watching
            .session
            .as_ref()
            .is_some_and(|(held, _)| held == session)
        {
            return;
        }
        if let Some((old, tokens)) = watching.session.take() {
            remove(&old, tokens);
        }
        let changed = watching.changed.clone();
        let (a, b, c) = (changed.clone(), changed.clone(), changed);
        let tokens = (|| -> windows::core::Result<[i64; 3]> {
            Ok([
                session.MediaPropertiesChanged(&TypedEventHandler::new(move |_, _| {
                    a();
                    Ok(())
                }))?,
                session.PlaybackInfoChanged(&TypedEventHandler::new(move |_, _| {
                    b();
                    Ok(())
                }))?,
                session.TimelinePropertiesChanged(&TypedEventHandler::new(move |_, _| {
                    c();
                    Ok(())
                }))?,
            ])
        })();
        if let Ok(tokens) = tokens {
            watching.session = Some((session.clone(), tokens));
        }
    }

    fn unwatch(&self) {
        let (manager, watching) = {
            let mut state = lock(&self.state);
            (state.manager.clone(), state.watching.take())
        };
        let Some(watching) = watching else {
            return;
        };
        if let Some(manager) = manager {
            let _ = manager.RemoveCurrentSessionChanged(watching.manager_token);
        }
        if let Some((session, tokens)) = watching.session {
            remove(&session, tokens);
        }
    }
}

fn remove(session: &Session, [properties, playback, timeline]: [i64; 3]) {
    let _ = session.RemoveMediaPropertiesChanged(properties);
    let _ = session.RemovePlaybackInfoChanged(playback);
    let _ = session.RemoveTimelinePropertiesChanged(timeline);
}

impl Drop for WindowsPlayer {
    fn drop(&mut self) {
        self.unwatch();
    }
}

/// 기본 출력 장치 볼륨. 장치가 바뀌어도 따라가도록 부를 때마다 새로 찾는다.
#[derive(Default)]
pub struct WindowsSpeaker;

impl Speaker for WindowsSpeaker {
    fn read(&self) -> Option<Level> {
        let volume = speaker()?;
        // SAFETY: 살아 있는 볼륨 조절기
        unsafe {
            let level = volume.GetMasterVolumeLevelScalar().ok()?;
            let muted = volume.GetMute().ok()?.as_bool();
            Some(Level {
                level: (level.clamp(0.0, 1.0) * 100.0).round() as u32,
                muted,
                settable: true,
            })
        }
    }

    fn set(&self, level: u32) -> Result<(), PortError> {
        let volume = speaker().ok_or_else(|| PortError::new("소리 장치가 없습니다"))?;
        let scalar = level.min(100) as f32 / 100.0;
        // SAFETY: 살아 있는 볼륨 조절기. 알림 문맥은 없음.
        unsafe { volume.SetMasterVolumeLevelScalar(scalar, std::ptr::null()) }
            .map_err(|_| PortError::new("볼륨을 바꾸지 못했습니다"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn winrt_times_turn_into_system_times() {
        let at = system_time(UNIX_EPOCH_TICKS + 10_000_000).unwrap();
        assert_eq!(at, UNIX_EPOCH + Duration::from_secs(1));
        assert_eq!(system_time(0), None);
        assert_eq!(system_time(i64::MAX), None, "미래 값");
        assert_eq!(millis(-5), 0);
        assert_eq!(millis(25_000_000), 2_500);
    }

    /// 패키지 없는(MSIX 아닌) 앱에서 시스템 미디어 세션 관리자를 열 수 있는지.
    #[test]
    #[ignore = "윈도우 시스템 미디어를 부른다. CI 윈도우 작업에서 돌린다"]
    fn windows_only_the_media_session_manager_opens_without_a_package() {
        let player = WindowsPlayer::default();
        assert!(player.manager().is_some(), "세션 관리자를 못 엶");
        let (track, _) = player.now().unwrap();
        eprintln!("지금 재생: {:?} {:?}", track.player, track.title);
        if track.player == MusicPlayer::None {
            assert!(player.command(MusicAction::PlayPause).is_err());
        }
        let pings = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = pings.clone();
        player.watch(Some(Arc::new(move || {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        })));
        player.watch(None);
    }

    #[test]
    #[ignore = "기본 출력 장치를 쓴다. CI 윈도우 작업에서 돌린다"]
    fn windows_only_volume_reads_and_sets_when_there_is_a_device() {
        let speaker = WindowsSpeaker;
        let Some(before) = speaker.read() else {
            eprintln!("소리 장치 없음");
            assert!(speaker.set(10).is_err());
            return;
        };
        assert!(before.level <= 100);
        let other = if before.level == 40 { 60 } else { 40 };
        speaker.set(other).unwrap();
        assert_eq!(speaker.read().unwrap().level, other);
        speaker.set(before.level).unwrap();
    }
}
