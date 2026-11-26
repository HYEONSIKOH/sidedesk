//! COM 준비와 기본 출력 장치 볼륨. 시험은 main 을 거치지 않으므로 쓰는 곳에서 늦게 연다.

use std::sync::OnceLock;

use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{IMMDeviceEnumerator, MMDeviceEnumerator, eConsole, eRender};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance,
    CoIncrementMTAUsage, CoInitializeEx, CoUninitialize,
};

/// 프로세스에 MTA 를 하나 열어 둔다. 따로 초기화하지 않은 우리 스레드도 MTA 로 COM 을 쓴다.
/// 받은 쿠키는 프로세스가 끝날 때까지 놓지 않는다.
pub(crate) fn mta() {
    static OPENED: OnceLock<bool> = OnceLock::new();
    // SAFETY: 인자 없는 호출. 몇 번 불러도 되지만 OnceLock 으로 한 번만.
    OPENED.get_or_init(|| unsafe { CoIncrementMTAUsage() }.is_ok());
}

/// 셸 호출(ShellExecuteEx, 셸 폴더, 고르기 창)은 STA 에서 부르라고 MS 가 권한다. 이 스레드를 잠시 STA 로.
/// 이미 MTA 로 정해진 스레드면 바꾸지 못하고 그대로 쓴다(놓지도 않음).
pub(crate) struct Sta(bool);

impl Sta {
    pub(crate) fn enter() -> Self {
        // SAFETY: 이 스레드의 COM 초기화. 성공했을 때만 Drop 에서 짝을 맞춘다.
        let entered =
            unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) }
                .is_ok();
        Self(entered)
    }
}

impl Drop for Sta {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: enter 에서 성공한 초기화와 짝
            unsafe { CoUninitialize() };
        }
    }
}

/// 기본 출력 장치의 볼륨 조절기. 장치가 바뀌어도 따라가도록 부를 때마다 새로 찾는다.
pub(crate) fn speaker() -> Option<IAudioEndpointVolume> {
    mta();
    // SAFETY: MTA 안에서 시스템 COM 객체를 만들고 쓴다. 실패하면 없음.
    unsafe {
        let devices: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let device = devices.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        device
            .Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)
            .ok()
    }
}
