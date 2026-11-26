import Observation

/// 화면이 보는 연결 값. 연결 스레드는 메인 액터로만 여기에 쓴다.
@MainActor
@Observable
final class LinkFeed {
    var state = LinkState.waiting
    /// 블루투스를 쓸 수 없는 이유. USB 는 그대로 쓴다.
    var bluetoothProblem: String?
    /// 마지막으로 받은 값. 끊겨도 지우지 않는다.
    var perf: Perf?
    var processes: [ComputerProcess]?
    var weather: Weather?
    /// 블루투스 상태를 알게 됐다(권한 창이 닫혔다). 위치 안내는 그 뒤에 띄운다.
    var bluetoothDecided = false

    @ObservationIgnored private var perfListeners: [(Perf) -> Void] = []
    @ObservationIgnored private var killListeners: [(KillResult) -> Void] = []

    init() {}

    /// 새 성능 값이 올 때마다(같은 값이어도) 부른다. 그래프가 쓴다.
    func onPerf(_ listener: @escaping (Perf) -> Void) {
        perfListeners.append(listener)
    }

    func onKillResult(_ listener: @escaping (KillResult) -> Void) {
        killListeners.append(listener)
    }

    func receive(perf value: Perf) {
        perf = value
        for listener in perfListeners {
            listener(value)
        }
    }

    func emit(_ result: KillResult) {
        for listener in killListeners {
            listener(result)
        }
    }
}

@MainActor
protocol ComputerLink: AnyObject {
    var feed: LinkFeed { get }

    func start()
    func stop()
    /// 연결이 바뀌어도 마지막 구독을 다시 보낸다.
    func subscribe(_ subscription: Subscription)
    /// 연결이 바뀌어도 마지막 위치를 다시 보낸다. off 면 받아 둔 날씨도 지운다.
    func setLocation(_ location: LocationState)
    /// 결과가 올 때까지 다시 이어질 때마다 같은 번호로 보낸다.
    func kill(_ command: KillCommand)
    /// 기한 안에 결과가 없으면 부른다. 아직 남아 있으면 지우고 실패로 알린다.
    func giveUp(_ commandId: UInt64)
    /// 등록 번호를 확인한 결과
    func answer(_ same: Bool)
}
