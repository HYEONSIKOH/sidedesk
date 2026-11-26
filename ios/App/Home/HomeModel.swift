import Observation

/// 짝 맺기 숫자, 문제, 기다림을 보이는 첫 화면(안드로이드 `HomeViewModel`)
@MainActor
@Observable
final class HomeModel {
    @ObservationIgnored private let link: any ComputerLink

    init(link: any ComputerLink) {
        self.link = link
    }

    var computerName: String? {
        switch link.feed.state {
        case .connected(let name, _), .lost(let name): name
        default: nil
        }
    }

    /// 끊겨서 마지막 값을 흐리게 보인다
    var dimmed: Bool {
        if case .lost = link.feed.state { true } else { false }
    }

    var pairingCode: String? {
        if case .pairing(let code) = link.feed.state { code } else { nil }
    }

    var problem: String? {
        if case .failed(let reason) = link.feed.state { reason } else { nil }
    }

    /// 블루투스를 쓸 수 없는 이유. USB 는 된다.
    var notice: String? { link.feed.bluetoothProblem }

    func answer(_ same: Bool) {
        link.answer(same)
    }
}
