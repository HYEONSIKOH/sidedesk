import Foundation

/// 화면에 보낼 값 한 벌
struct LinkSnapshot: Sendable, Equatable {
    var state: LinkState
    var perf: Perf?
    var processes: [ComputerProcess]?
    var weather: Weather?
}

/// 지금 화면을 쓰는 연결 하나를 정한다. USB 와 블루투스 받는 쪽이 같이 쓴다(안드로이드 `LinkOwner` 와 같은 규칙).
/// - 아이폰 USB 는 접속한 쪽을 확인할 수 없으므로 블루투스처럼 Noise 가 상대를 확인한 뒤(숫자나 Ready) 이어받는다.
///   이어받을 때는 블루투스를 이기고 이전 USB 를 바꾼다.
/// - 블루투스는 Ready 뒤에만 이어받고, USB 가 살아 있거나 다른 컴퓨터가 쓰고 있으면 진다.
///
/// 잠금 하나로 모든 값을 지킨다. 잠금 안에서는 남의 잠금을 잡지 않는다(닫기 함수는 소켓을 끊기만 한다).
final class LinkOwner: @unchecked Sendable {
    static let killLimit: TimeInterval = 30
    static let notConnected = "컴퓨터와 연결되어 있지 않습니다"

    private struct Pending {
        let command: KillCommand
        let computer: Data
        let at: TimeInterval
    }

    private final class Holder {
        let via: Via
        var computer: Data?
        let close: @Sendable () -> Void
        let generation: Int

        init(via: Via, computer: Data?, close: @escaping @Sendable () -> Void, generation: Int) {
            self.via = via
            self.computer = computer
            self.close = close
            self.generation = generation
        }
    }

    private let lock = NSLock()
    private let clock: @Sendable () -> TimeInterval
    private let publish: @Sendable (LinkSnapshot) -> Void
    private let emitKill: @Sendable (KillResult) -> Void

    private var snapshot = LinkSnapshot(state: .waiting)
    private var pendingAnswer: Bool?
    private var name: String?
    private var location = LocationState.unknown
    private var lastSubscription = Subscription.none
    private var kills: [UInt64: Pending] = [:]
    private var killOrder: [UInt64] = []
    /// 마지막으로 키를 확인한 컴퓨터. 끊긴 뒤에도 남겨 같은 컴퓨터가 다시 붙으면 밀린 명령을 보낸다.
    private var lastComputer: Data?
    private var generation = 0
    private var current: Holder?
    /// 앱이 뒤로 갔다. 확인 중이던 접속이 이어받지 못하게 한다.
    private var paused = false

    init(
        clock: @escaping @Sendable () -> TimeInterval = { ProcessInfo.processInfo.systemUptime },
        publish: @escaping @Sendable (LinkSnapshot) -> Void = { _ in },
        emitKill: @escaping @Sendable (KillResult) -> Void = { _ in }
    ) {
        self.clock = clock
        self.publish = publish
        self.emitKill = emitKill
    }

    private func locked<T>(_ work: () -> T) -> T {
        lock.lock()
        defer { lock.unlock() }
        return work()
    }

    /// 바꾼 뒤 화면으로 보낸다.
    private func changing(_ work: () -> Bool) {
        let next: LinkSnapshot? = locked {
            work() ? snapshot : nil
        }
        if let next {
            publish(next)
        }
    }

    // MARK: 읽기 (시험과 연결이 쓴다)

    var state: LinkState { locked { snapshot.state } }
    var perf: Perf? { locked { snapshot.perf } }
    var processes: [ComputerProcess]? { locked { snapshot.processes } }
    var weather: Weather? { locked { snapshot.weather } }
    var lastName: String? {
        get { locked { name } }
        set { locked { name = newValue } }
    }
    var subscription: Subscription {
        get { locked { lastSubscription } }
        set { locked { lastSubscription = newValue } }
    }
    var currentLocation: LocationState { locked { location } }

    func setAnswer(_ same: Bool) {
        locked { pendingAnswer = same }
    }

    func takeAnswer() -> Bool? {
        locked {
            defer { pendingAnswer = nil }
            return pendingAnswer
        }
    }

    // MARK: 이어받기

    func pause(_ value: Bool) {
        locked { paused = value }
    }

    /// 이어받았으면 세대 번호, 앱이 뒤에 있으면 nil
    func takeUsb(close: @escaping @Sendable () -> Void) -> Int? {
        let taken: (Int, Holder?)? = locked {
            if paused { return nil }
            let old = current
            generation += 1
            current = Holder(via: .usb, computer: nil, close: close, generation: generation)
            pendingAnswer = nil
            return (generation, old)
        }
        guard let (mine, old) = taken else { return nil }
        old?.close()
        return mine
    }

    /// 이어받았으면 세대 번호, 졌으면 nil
    func takeBluetooth(computer: Data, close: @escaping @Sendable () -> Void) -> Int? {
        let taken: (Int, Holder?)? = locked {
            if paused { return nil }
            if let now = current {
                if now.via == .usb { return nil }
                if let other = now.computer, other != computer { return nil }
            }
            let old = current
            generation += 1
            current = Holder(via: .bluetooth, computer: computer, close: close, generation: generation)
            lastComputer = computer
            pendingAnswer = nil
            return (generation, old)
        }
        guard let (mine, old) = taken else { return nil }
        old?.close()
        return mine
    }

    func ready(_ mine: Int, computer: Data) {
        locked {
            guard let now = current, now.generation == mine else { return }
            now.computer = computer
            lastComputer = computer
        }
    }

    func release(_ mine: Int) {
        locked {
            if current?.generation == mine { current = nil }
        }
    }

    func isCurrent(_ mine: Int) -> Bool {
        locked { current?.generation == mine }
    }

    /// 화면 상태는 지금 연결만 바꾼다.
    func show(_ mine: Int, _ next: LinkState) {
        changing {
            guard generation == mine, snapshot.state != next else { return false }
            snapshot.state = next
            return true
        }
    }

    /// 아무 연결도 없을 때만 보인다(확인 전 아이폰 USB 의 실패 이유).
    func showIfIdle(_ next: LinkState) {
        changing {
            guard current == nil, snapshot.state != next else { return false }
            snapshot.state = next
            return true
        }
    }

    func closeAll() {
        let close = locked { current?.close }
        close?()
    }

    // MARK: 종료 명령

    /// 지금 컴퓨터 앞으로 명령을 둔다. 아는 컴퓨터가 없으면 바로 실패로 알린다.
    func kill(_ command: KillCommand) {
        let queued: Bool = locked {
            guard let computer = lastComputer else { return false }
            kills[command.commandId] = Pending(command: command, computer: computer, at: clock())
            killOrder.removeAll { $0 == command.commandId }
            killOrder.append(command.commandId)
            return true
        }
        if !queued {
            emitKill(KillResult(commandId: command.commandId, ok: false, reason: Self.notConnected))
        }
    }

    /// 이 컴퓨터 앞으로 둔, 기한이 지나지 않은 명령
    func pendingKills(computer: Data) -> [KillCommand] {
        locked {
            let now = clock()
            return killOrder.compactMap { kills[$0] }
                .filter { $0.computer == computer && now - $0.at <= Self.killLimit }
                .map(\.command)
        }
    }

    /// 기한 안에 결과가 오지 않은 명령을 지우고 실패로 알린다.
    func giveUp(_ commandId: UInt64) {
        if forget(commandId) {
            emitKill(KillResult(commandId: commandId, ok: false, reason: Self.notConnected))
        }
    }

    /// 명령 결과는 어느 연결로 왔든 버리지 않는다.
    func receivedKill(_ result: KillResult) {
        if forget(result.commandId) {
            emitKill(result)
        }
    }

    private func forget(_ commandId: UInt64) -> Bool {
        locked {
            killOrder.removeAll { $0 == commandId }
            return kills.removeValue(forKey: commandId) != nil
        }
    }

    // MARK: 받은 값 (지금 연결 것만)

    func receivedPerf(_ mine: Int, _ value: Perf) {
        changing {
            guard generation == mine else { return false }
            snapshot.perf = value
            return true
        }
    }

    func receivedProcesses(_ mine: Int, _ value: [ComputerProcess]) {
        changing {
            guard generation == mine else { return false }
            snapshot.processes = value
            return true
        }
    }

    func receivedWeather(_ mine: Int, _ value: Weather) {
        changing {
            guard generation == mine, location != .off else { return false }
            snapshot.weather = value
            return true
        }
    }

    /// 권한을 거두면 다른 곳의 날씨를 흐리게 남기지 않도록 지운다.
    func setLocation(_ value: LocationState) {
        changing {
            location = value
            guard value == .off, snapshot.weather != nil else { return false }
            snapshot.weather = nil
            return true
        }
    }
}
