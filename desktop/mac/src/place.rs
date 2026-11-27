//! 이 맥의 대략적인 위치(소수 첫째 자리). 날씨를 이 위치로 받는다.
//! 위치 관리자는 주 스레드에서 만들고 읽는다(주 스레드의 실행 고리로 값이 들어온다).
//! 처음 쓸 때 macOS 가 위치 허용을 묻는다. 거절하면 계속 없음(그때는 폰 위치를 쓴다).

use std::sync::{Mutex, MutexGuard};

use dispatch2::DispatchQueue;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_core_location::{CLLocationManager, kCLLocationAccuracyThreeKilometers};
use sidedesk_domain::{HerePlace, Place};

struct Manager(Retained<CLLocationManager>);

// SAFETY: 관리자는 아래 `on_main` 안에서만, 곧 주 스레드에서만 만지고 읽는다.
unsafe impl Send for Manager {}

#[derive(Default)]
pub struct MacPlace {
    manager: Mutex<Option<Manager>>,
}

impl MacPlace {
    fn manager(&self) -> MutexGuard<'_, Option<Manager>> {
        self.manager.lock().unwrap_or_else(|l| l.into_inner())
    }
}

/// 주 스레드에서 돌린다. 이미 주 스레드면 바로(exec_sync 로 자기를 기다리면 멈춘다).
fn on_main(work: impl FnOnce() + Send) {
    if MainThreadMarker::new().is_some() {
        work();
    } else {
        DispatchQueue::main().exec_sync(work);
    }
}

/// 도(degree)를 소수 첫째 자리 정수로. 범위 밖이거나 (0, 0)이면 없음.
fn tenths(latitude: f64, longitude: f64) -> Option<Place> {
    if !latitude.is_finite() || !longitude.is_finite() {
        return None;
    }
    let lat = (latitude * 10.0).round();
    let lon = (longitude * 10.0).round();
    ((-900.0..=900.0).contains(&lat) && (-1800.0..=1800.0).contains(&lon) && (lat, lon) != (0.0, 0.0))
        .then_some(Place {
            latitude: lat as i16,
            longitude: lon as i16,
        })
}

impl HerePlace for MacPlace {
    fn read(&self) -> Option<Place> {
        let mut guard = self.manager();
        let slot: &mut Option<Manager> = &mut guard;
        let mut found = None;
        on_main(|| {
            let manager = slot.get_or_insert_with(|| {
                // SAFETY: 주 스레드에서 만들고, 정확도를 낮춘 뒤 허용을 묻고 위치 받기를 켠다.
                let manager = unsafe { CLLocationManager::new() };
                unsafe {
                    manager.setDesiredAccuracy(kCLLocationAccuracyThreeKilometers);
                    manager.requestWhenInUseAuthorization();
                    manager.startUpdatingLocation();
                }
                Manager(manager)
            });
            // SAFETY: 주 스레드에서 마지막으로 받은 위치를 읽는다.
            found = unsafe { manager.0.location() }.and_then(|location| {
                let point = unsafe { location.coordinate() };
                tenths(point.latitude, point.longitude)
            });
            if found.is_none() {
                // 처음에 거부됐다가 나중에 허용하면 받기가 꺼져 있으므로 다시 켠다.
                unsafe { manager.0.startUpdatingLocation() };
            }
        });
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounds_to_tenths_and_rejects_nonsense() {
        assert_eq!(tenths(37.5665, 126.978), Some(Place { latitude: 376, longitude: 1270 }));
        assert_eq!(tenths(-33.86, 151.21), Some(Place { latitude: -339, longitude: 1512 }));
        assert_eq!(tenths(0.0, 0.0), None, "위치 없음");
        assert_eq!(tenths(f64::NAN, 1.0), None);
        assert_eq!(tenths(95.0, 1.0), None);
    }
}
