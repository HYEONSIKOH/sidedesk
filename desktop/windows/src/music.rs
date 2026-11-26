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

#[cfg(test)]
mod probe {
    use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager;

    /// 패키지 없는(MSIX 아닌) 앱에서 시스템 미디어 세션 관리자를 열 수 있는지 먼저 본다.
    #[test]
    #[ignore = "윈도우 시스템 미디어를 부른다. CI 윈도우 작업에서 돌린다"]
    fn windows_only_the_media_session_manager_opens_without_a_package() {
        let manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
            .expect("요청")
            .join()
            .expect("세션 관리자");
        let current = manager.GetCurrentSession();
        eprintln!("지금 세션 있음: {}", current.is_ok());
    }
}
