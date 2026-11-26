import Foundation

/// 화면에 보낼 값 한 벌
struct LinkSnapshot: Sendable, Equatable {
    var state: LinkState
    var perf: Perf?
    var processes: [ComputerProcess]?
    var weather: Weather?
    var sessions: [AiSession]?
    var forge: Forge?
    var shortcuts: [Shortcut]?
    var nowPlaying: NowPlaying?
    var volume: Volume?
    /// 해시로 기억한 앨범 그림(jpeg). 마지막 8장
    var artworks: [UInt64: Data] = [:]
}

/// 지금 화면을 쓰는 연결 하나를 정한다. USB 와 블루투스 받는 쪽이 같이 쓴다(안드로이드 `LinkOwner` 와 같은 규칙).
/// - 아이폰 USB 는 접속한 쪽을 확인할 수 없으므로 블루투스처럼 Noise 가 상대를 확인한 뒤(숫자나 Ready) 이어받는다.
///   이어받을 때는 블루투스를 이기고 이전 USB 를 바꾼다.
/// - 블루투스는 Ready 뒤에만 이어받고, USB 가 살아 있거나 다른 컴퓨터가 쓰고 있으면 진다.
///
/// 잠금 하나로 모든 값을 지킨다. 잠금 안에서는 남의 잠금을 잡지 않는다(닫기 함수는 소켓을 끊기만 한다).
final class LinkOwner: @unchecked Sendable {
    static let killLimit: TimeInterval = 30
    static let artworkLimit = 8
    static let notConnected = "컴퓨터와 연결되어 있지 않습니다"

    private struct Pending {
        let command: Command
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
    /// 음악 받은 때를 적는 시계(부팅 뒤 밀리초, 잠든 시간 포함)
    private let elapsed: @Sendable () -> Int64
    private let publish: @Sendable (LinkSnapshot) -> Void
    private let emitResult: @Sendable (CommandResult) -> Void
    private let emitAlert: @Sendable (AiAlert) -> Void

    private var snapshot = LinkSnapshot(state: .waiting)
    private var pendingAnswer: Bool?
    private var name: String?
    private var location = LocationState.unknown
    private var lastSubscription = Subscription.none
    private var kills: [UInt64: Pending] = [:]
    private var killOrder: [UInt64] = []
    /// 단축키 목록을 보낸 컴퓨터와 장부 번호. 번호는 컴퓨터마다 따로라 다른 컴퓨터에 보내지 않는다.
    private var shortcutsFrom: Data?
    private var shortcutsBook: UInt64 = 0
    /// 곡과 볼륨을 보낸 컴퓨터. 다른 컴퓨터에 음악 명령을 보내지 않는다.
    private var musicFrom: Data?
    /// 바로 보낼 볼륨 하나. 다시 붙어도 보내지 않는다(늦은 값이 새 값을 덮지 않게).
    private var volumeOut: MusicCommand?
    private var volumeFor = 0
    /// 결과를 기다리는 볼륨 명령 번호. 다시 보내지는 않지만 실패는 넘긴다.
    private var volumeWaiting = Set<UInt64>()
    private var artworkOrder: [UInt64] = []
    /// 이 세대에서 받은 알림 번호. 새 연결이면 비운다(컴퓨터가 다시 켜지면 번호가 1부터 다시 간다).
    private var seenAlerts = Set<UInt64>()
    private var seenFor = 0
    /// 마지막으로 키를 확인한 컴퓨터. 끊긴 뒤에도 남겨 같은 컴퓨터가 다시 붙으면 밀린 명령을 보낸다.
    private var lastComputer: Data?
    private var generation = 0
    private var current: Holder?
    /// 앱이 뒤로 갔다. 확인 중이던 접속이 이어받지 못하게 한다.
    private var paused = false

    init(
        clock: @escaping @Sendable () -> TimeInterval = { ProcessInfo.processInfo.systemUptime },
        elapsed: @escaping @Sendable () -> Int64 = { monotonicMillis() },
        publish: @escaping @Sendable (LinkSnapshot) -> Void = { _ in },
        emitResult: @escaping @Sendable (CommandResult) -> Void = { _ in },
        emitAlert: @escaping @Sendable (AiAlert) -> Void = { _ in }
    ) {
        self.clock = clock
        self.elapsed = elapsed
        self.publish = publish
        self.emitResult = emitResult
        self.emitAlert = emitAlert
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
    var sessions: [AiSession]? { locked { snapshot.sessions } }
    var forge: Forge? { locked { snapshot.forge } }
    var shortcuts: [Shortcut]? { locked { snapshot.shortcuts } }
    var nowPlaying: NowPlaying? { locked { snapshot.nowPlaying } }
    var volume: Volume? { locked { snapshot.volume } }
    var artworks: [UInt64: Data] { locked { snapshot.artworks } }
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
            forgetOthers(computer)
            return (generation, old)
        }
        guard let (mine, old) = taken else { return nil }
        old?.close()
        publishNow()
        return mine
    }

    func ready(_ mine: Int, computer: Data) {
        changing {
            guard let now = current, now.generation == mine else { return false }
            now.computer = computer
            lastComputer = computer
            return forgetOthers(computer)
        }
    }

    /// 다른 컴퓨터가 붙으면 앞 컴퓨터의 단축키 목록과 곡을 바로 지운다. 잠금 안에서 부른다.
    @discardableResult
    private func forgetOthers(_ computer: Data) -> Bool {
        var changed = false
        if let music = musicFrom, music != computer {
            snapshot.nowPlaying = nil
            snapshot.volume = nil
            musicFrom = nil
            changed = true
        }
        if let from = shortcutsFrom, from != computer {
            snapshot.shortcuts = nil
            shortcutsFrom = nil
            changed = true
        }
        return changed
    }

    private func publishNow() {
        publish(locked { snapshot })
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
    func send(_ command: Command) {
        let queued: Bool = locked {
            guard let computer = lastComputer else { return false }
            kills[command.commandId] = Pending(command: command, computer: computer, at: clock())
            killOrder.removeAll { $0 == command.commandId }
            killOrder.append(command.commandId)
            return true
        }
        if !queued {
            emitResult(CommandResult(commandId: command.commandId, ok: false, reason: Self.notConnected))
        }
    }

    /// 이 컴퓨터 앞으로 둔, 기한이 지나지 않은 명령
    func pendingCommands(computer: Data) -> [Command] {
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
            emitResult(CommandResult(commandId: commandId, ok: false, reason: Self.notConnected))
        }
    }

    /// 명령 결과는 어느 연결로 왔든 버리지 않는다.
    func receivedResult(_ result: CommandResult) {
        let known = locked { volumeWaiting.remove(result.commandId) != nil }
        if forget(result.commandId) || known {
            emitResult(result)
        }
    }

    private func forget(_ commandId: UInt64) -> Bool {
        locked {
            killOrder.removeAll { $0 == commandId }
            return kills.removeValue(forKey: commandId) != nil
        }
    }

    // MARK: 단축키와 음악

    /// 지금 붙은 컴퓨터가 보낸 목록에 있는 단축키만 명령으로 둔다. 아니면 보내지 않고 false.
    func runShortcut(_ command: RunShortcut) -> Bool {
        let booked: RunShortcut? = locked {
            guard let from = shortcutsFrom, let now = current?.computer, from == now,
                  snapshot.shortcuts?.contains(where: { $0.id == command.shortcutId }) == true
            else { return nil }
            var booked = command
            booked.book = shortcutsBook
            return booked
        }
        guard let booked else { return false }
        send(.shortcut(booked))
        return true
    }

    /// 보이는 곡(볼륨)이 지금 붙은 컴퓨터 것일 때만 명령을 둔다.
    /// 재생/멈춤, 다음, 이전은 대기열(같은 번호 다시 보냄), 볼륨은 바로 보낼 칸에 마지막 값만 둔다.
    func music(_ command: MusicCommand) -> Bool {
        let ok: Bool = locked {
            guard let from = musicFrom, let holder = current, let now = holder.computer, from == now else { return false }
            if command.action == .setVolume {
                volumeOut = command
                volumeFor = holder.generation
                volumeWaiting.insert(command.commandId)
                return true
            }
            return (snapshot.nowPlaying?.player ?? .none) != .none
        }
        if ok, command.action != .setVolume { send(.music(command)) }
        return ok
    }

    /// 이 연결에 보낼 볼륨. 다른 연결에서 든 값은 버린다.
    func takeVolume(_ mine: Int) -> MusicCommand? {
        locked {
            guard let out = volumeOut else { return nil }
            volumeOut = nil
            return volumeFor == mine ? out : nil
        }
    }

    func receivedShortcuts(_ mine: Int, book: UInt64, _ value: [Shortcut]) {
        changing {
            guard let now = current, now.generation == mine, let computer = now.computer else { return false }
            snapshot.shortcuts = value
            shortcutsFrom = computer
            shortcutsBook = book
            return true
        }
    }

    /// 받은 순간의 폰 시각을 붙여 둔다.
    func receivedNowPlaying(_ mine: Int, _ value: NowPlaying) {
        changing {
            guard let now = current, now.generation == mine, let computer = now.computer else { return false }
            var stamped = value
            stamped.receivedAt = elapsed()
            snapshot.nowPlaying = stamped
            musicFrom = computer
            return true
        }
    }

    func receivedVolume(_ mine: Int, _ value: Volume) {
        changing {
            guard let now = current, now.generation == mine, let computer = now.computer else { return false }
            snapshot.volume = value
            musicFrom = computer
            return true
        }
    }

    func receivedArtwork(_ mine: Int, hash: UInt64, jpeg: Data) {
        changing {
            guard generation == mine, hash != 0 else { return false }
            artworkOrder.removeAll { $0 == hash }
            artworkOrder.append(hash)
            snapshot.artworks[hash] = jpeg
            while artworkOrder.count > Self.artworkLimit {
                snapshot.artworks.removeValue(forKey: artworkOrder.removeFirst())
            }
            return true
        }
    }

    // MARK: AI

    func receivedSessions(_ mine: Int, _ value: [AiSession]) {
        changing {
            guard generation == mine else { return false }
            snapshot.sessions = value
            return true
        }
    }

    func receivedForge(_ mine: Int, _ value: Forge) {
        changing {
            guard generation == mine else { return false }
            snapshot.forge = value
            return true
        }
    }

    /// 지금 연결의 알림만, 같은 번호는 한 번만 넘긴다.
    func receivedAlert(_ mine: Int, _ value: AiAlert) {
        let fresh: Bool = locked {
            guard generation == mine else { return false }
            if seenFor != mine {
                seenAlerts.removeAll()
                seenFor = mine
            }
            return seenAlerts.insert(value.id).inserted
        }
        if fresh { emitAlert(value) }
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
