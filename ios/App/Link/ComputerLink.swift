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

@MainActor
func makeLink() -> any ComputerLink {
    #if DEBUG && targetEnvironment(simulator)
    DevLink()
    #else
    NoLink()
    #endif
}

#if !(DEBUG && targetEnvironment(simulator))
private final class NoLink: ComputerLink {
    let state = LinkState.waiting

    func start() {}
    func stop() {}
}
#endif
