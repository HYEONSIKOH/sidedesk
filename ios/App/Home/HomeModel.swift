import Observation

@MainActor
@Observable
final class HomeModel {
    private let link: any ComputerLink

    init(link: any ComputerLink) {
        self.link = link
    }

    var computerName: String? {
        if case .received(let name) = link.state { name } else { nil }
    }

    var problem: String? {
        if case .failed(let reason) = link.state { reason } else { nil }
    }

    func start() {
        link.start()
    }

    func stop() {
        link.stop()
    }
}
