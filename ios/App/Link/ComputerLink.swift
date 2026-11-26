enum LinkState: Equatable, Sendable {
    case waiting
    case received(String)
    case failed(String)
}

@MainActor
protocol ComputerLink: AnyObject {
    var state: LinkState { get }

    func start()
    func stop()
}

/// 아이폰 연결은 아직 없다(설계 6단계).
@MainActor
func makeLink() -> any ComputerLink {
    NoLink()
}

private final class NoLink: ComputerLink {
    let state = LinkState.waiting

    func start() {}
    func stop() {}
}
