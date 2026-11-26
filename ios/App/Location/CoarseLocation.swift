import CoreLocation
import Foundation

/// 위치 권한과 읽기. 시험에서는 가짜를 넣는다.
@MainActor
protocol LocationPermission: AnyObject {
    /// 아직 묻지 않았다
    var undecided: Bool { get }
    /// 시스템 권한 창을 띄운다.
    func ask()
    func refresh()
    func stop()
}

/// 앱이 앞에 올 때 대략적인 위치를 한 번 읽어 컴퓨터로 넘긴다. 반올림은 core 가 한다.
/// 관리자는 메인 스레드에서 만들어 콜백도 메인 스레드로 온다.
@MainActor
final class CoarseLocation: NSObject, CLLocationManagerDelegate, LocationPermission {
    static let timeout: Duration = .seconds(30)

    private let link: any ComputerLink
    private let manager = CLLocationManager()
    private var waiting: Task<Void, Never>?

    init(link: any ComputerLink) {
        self.link = link
        super.init()
        manager.delegate = self
        manager.desiredAccuracy = kCLLocationAccuracyReduced
    }

    var undecided: Bool { manager.authorizationStatus == .notDetermined }

    /// 안내 창에서 "다음"을 누른 뒤에 시스템 권한 창을 띄운다.
    func ask() {
        manager.requestWhenInUseAuthorization()
    }

    /// 권한이 없으면 위치 없음. 아직 묻지 않았으면 그대로 둔다.
    func refresh() {
        switch manager.authorizationStatus {
        case .denied, .restricted:
            link.setLocation(.off)
        case .authorizedWhenInUse, .authorizedAlways:
            stop()
            manager.requestLocation()
            // 시간이 넘으면 마지막 위치로 대신한다.
            waiting = Task { [weak self] in
                try? await Task.sleep(for: Self.timeout)
                guard !Task.isCancelled, let self else { return }
                use(manager.location)
            }
        default:
            break
        }
    }

    func stop() {
        waiting?.cancel()
        waiting = nil
    }

    private func use(_ found: CLLocation?) {
        stop()
        guard let place = found ?? manager.location else { return }
        link.setLocation(.at(latitude: place.coordinate.latitude, longitude: place.coordinate.longitude))
    }

    nonisolated func locationManagerDidChangeAuthorization(_ manager: CLLocationManager) {
        MainActor.assumeIsolated { refresh() }
    }

    nonisolated func locationManager(_ manager: CLLocationManager, didUpdateLocations locations: [CLLocation]) {
        let last = locations.last
        MainActor.assumeIsolated { use(last) }
    }

    nonisolated func locationManager(_ manager: CLLocationManager, didFailWithError error: Error) {
        MainActor.assumeIsolated { use(nil) }
    }
}
