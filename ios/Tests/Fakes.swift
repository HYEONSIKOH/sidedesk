import Foundation

@testable import SideDesk

/// 화면 모델 시험용 연결. 부른 것을 적어 둔다.
@MainActor
final class FakeLink: ComputerLink {
    let feed = LinkFeed()
    var subscriptions: [Subscription] = []
    var locations: [LocationState] = []
    var kills: [KillCommand] = []
    var givenUp: [UInt64] = []
    var answers: [Bool] = []
    var started = 0
    var stopped = 0

    func start() { started += 1 }
    func stop() { stopped += 1 }
    func subscribe(_ subscription: Subscription) { subscriptions.append(subscription) }
    func setLocation(_ location: LocationState) { locations.append(location) }
    func kill(_ command: KillCommand) { kills.append(command) }
    func giveUp(_ commandId: UInt64) { givenUp.append(commandId) }
    func answer(_ same: Bool) { answers.append(same) }
}

@MainActor
final class MemorySettings: Settings {
    var face: String?
    var theme: String?
    var askedLocation = false

    init(face: String? = nil, theme: String? = nil) {
        self.face = face
        self.theme = theme
    }
}

final class MemoryKeys: KeyStore, @unchecked Sendable {
    private let lock = NSLock()
    private var keys: [Data] = []
    let me = Identity(privateKey: Data(repeating: 1, count: 32), publicKey: Data(repeating: 2, count: 32))

    init(computers: [Data] = []) {
        keys = computers
    }

    func identity() throws -> Identity { me }

    func computers() throws -> [Data] {
        lock.lock()
        defer { lock.unlock() }
        return keys
    }

    func trust(_ key: Data) throws {
        lock.lock()
        defer { lock.unlock() }
        if !keys.contains(key) { keys.append(key) }
    }
}

/// 손으로 돌리는 시계
final class Clock: @unchecked Sendable {
    private let lock = NSLock()
    private var value: TimeInterval = 1000

    var now: TimeInterval {
        lock.lock()
        defer { lock.unlock() }
        return value
    }

    func advance(_ seconds: TimeInterval) {
        lock.lock()
        value += seconds
        lock.unlock()
    }
}

/// 대본대로 받는 통로. 받을 것이 떨어지면 시계를 조금씩 돌리며 빈 값을 낸다.
final class FakePipe: LinkPipe, @unchecked Sendable {
    private let lock = NSLock()
    private var incoming: [Data]
    private let clock: Clock
    /// 받을 것이 떨어진 뒤 끊을지(아니면 빈 값을 계속 낸다)
    private let endWhenEmpty: Bool
    private(set) var sent: [Data] = []
    private(set) var closed = false

    init(_ incoming: [Data], clock: Clock, endWhenEmpty: Bool = true) {
        self.incoming = incoming
        self.clock = clock
        self.endWhenEmpty = endWhenEmpty
    }

    func receive() throws -> Data? {
        lock.lock()
        defer { lock.unlock() }
        if closed { return nil }
        if !incoming.isEmpty { return incoming.removeFirst() }
        if endWhenEmpty { return nil }
        clock.advance(1)
        return Data()
    }

    func send(_ bytes: Data) throws {
        lock.lock()
        defer { lock.unlock() }
        sent.append(bytes)
    }

    func close() {
        lock.lock()
        closed = true
        lock.unlock()
    }

    var isClosed: Bool {
        lock.lock()
        defer { lock.unlock() }
        return closed
    }
}

/// 받은 바이트 한 덩이마다 정해 둔 일을 낸다. 보낸 것을 적어 둔다.
final class FakeSession: CoreSession {
    /// feed 할 때마다 하나씩 꺼낸다. 오류면 그 feed 가 던진다.
    var script: [Result<[CoreEvent], CoreError>]
    private var events: [CoreEvent] = []
    var subscribed: [(Topic?, [Topic])] = []
    var located: [String] = []
    var killed: [UInt64] = []
    var confirmed: [Bool] = []

    init(_ script: [Result<[CoreEvent], CoreError>]) {
        self.script = script
    }

    func feed(_ bytes: Data) throws {
        guard !script.isEmpty else { return }
        switch script.removeFirst() {
        case .success(let next): events += next
        case .failure(let error): throw error
        }
    }

    func takeOutput() -> Data { Data() }

    func nextEvent() -> CoreEvent? {
        events.isEmpty ? nil : events.removeFirst()
    }

    func confirm(_ same: Bool) throws { confirmed.append(same) }

    func subscribe(main: Topic?, tiles: [Topic]) throws { subscribed.append((main, tiles)) }

    func sendLocation(latitude: Double, longitude: Double) throws { located.append("\(latitude),\(longitude)") }

    func sendNoLocation() throws { located.append("none") }

    func killProcess(requestId: UInt64, pid: Int32, startedAt: Int64) throws { killed.append(requestId) }
}

let mac = Data(repeating: 7, count: 32)
let otherMac = Data(repeating: 8, count: 32)
let chunk = Data([1])

func perf(cpu: Float = 10, down: Int64 = 0, up: Int64 = 0) -> Perf {
    Perf(cpu: cpu, gpu: nil, memory: 1, disk: 1, receivePerSecond: down, sendPerSecond: up)
}

func weather(fetchedAt: Int64 = 0, source: WeatherSource = .openMeteo) -> Weather {
    Weather(temperature: 3.4, sky: .clear, high: 5, low: -1, isDay: true, fetchedAt: fetchedAt, source: source)
}

func process(_ pid: Int32, _ name: String, cpu: Float = 0, memory: Int64 = 0) -> ComputerProcess {
    ComputerProcess(pid: pid, startedAt: 100, name: name, cpu: cpu, memoryBytes: memory)
}
