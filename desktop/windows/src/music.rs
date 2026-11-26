use sidedesk_core::MusicAction;
use sidedesk_domain::{Changed, Level, Picture, Player, PortError, Speaker, Track};

fn unsupported() -> PortError {
    PortError::new("윈도우에서는 아직 지원하지 않습니다")
}

/// 윈도우 음악 자리. win 이 WinRT 시스템 미디어로 채운다.
pub struct WindowsPlayer;

impl Player for WindowsPlayer {
    fn now(&self) -> Result<(Track, Option<Picture>), PortError> {
        Ok((Track::nothing(), None))
    }

    fn command(&self, _action: MusicAction) -> Result<(), PortError> {
        Err(unsupported())
    }

    fn watch(&self, _changed: Option<Changed>) {}
}

/// 윈도우 볼륨 자리. win 이 IAudioEndpointVolume 으로 채운다.
pub struct WindowsSpeaker;

impl Speaker for WindowsSpeaker {
    fn read(&self) -> Option<Level> {
        None
    }

    fn set(&self, _level: u32) -> Result<(), PortError> {
        Err(unsupported())
    }
}
