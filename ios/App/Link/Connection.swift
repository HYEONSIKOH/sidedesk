import Foundation
import os

/// 컴퓨터와 이어진 통로 하나. USB 와 블루투스가 이 모양으로 맞춘다.
protocol LinkPipe: AnyObject, Sendable {
    /// 받은 바이트. 잠깐 기다려도 없으면 빈 값, 끊겼으면 nil
    func receive() throws -> Data?
    func send(_ bytes: Data) throws
    /// 다른 스레드에서 불러도 된다. 기다리는 읽기를 깨운다.
    func close()
}

struct PipeError: Error {
    let reason: String
}

/// 공용 코어가 낸 일. 앱 형으로 옮겨 둔 것
enum CoreEvent: Equatable {
    case pairingCode(String)
    case paired(computerKey: Data)
    case ready(computerKey: Data)
    case computerName(String)
    case performance(Perf)
    case processes([ComputerProcess])
    case commandResult(requestId: UInt64, ok: Bool, reason: String)
    case weather(Weather)
}

enum CoreError: Error, Equatable {
    case version
    case notPaired
    case rejected
    case broken(String)
}

/// 접속 하나의 세션. 실제는 core 의 `PhoneSession`, 시험은 가짜.
protocol CoreSession: AnyObject {
    func feed(_ bytes: Data) throws
    func takeOutput() -> Data
    func nextEvent() -> CoreEvent?
    func confirm(_ same: Bool) throws
    func subscribe(main: Topic?, tiles: [Topic]) throws
    /// nil 은 위치 없음(권한 없음)
    func sendLocation(latitude: Double, longitude: Double) throws
    func sendNoLocation() throws
    func killProcess(requestId: UInt64, pid: Int32, startedAt: Int64) throws
}

struct Identity: Sendable, Equatable {
    let privateKey: Data
    let publicKey: Data
}

/// 이 폰의 키와 짝을 맺은 컴퓨터의 공개 키
protocol KeyStore: AnyObject, Sendable {
    func identity() throws -> Identity
    func computers() throws -> [Data]
    func trust(_ key: Data) throws
}

typealias SessionMaker = @Sendable (_ identity: Identity, _ computers: [Data], _ allowPairing: Bool) throws -> CoreSession

/// 통로 하나로 세션을 돌린다(안드로이드 `Connection` 과 같은 반복).
/// 세대 없이 시작해 USB 는 숫자나 Ready 때, 블루투스는 Ready 때 이어받는다.
final class Connection {
    static let silence: TimeInterval = 15
    private static let log = Logger(subsystem: "io.github.hyeonsikoh.sidedesk", category: "link")

    private let pipe: LinkPipe
    private let via: Via
    private let owner: LinkOwner
    private let keys: KeyStore
    private let makeSession: SessionMaker
    /// 이 시간 안에 숫자나 Ready 가 나오지 않으면 닫는다.
    private let readyLimit: TimeInterval
    /// 이어받은 뒤 부른다(확인 자리를 놓는다).
    private let afterTake: () -> Void
    private let clock: () -> TimeInterval
    private var mine: Int?

    init(
        pipe: LinkPipe,
        via: Via,
        owner: LinkOwner,
        keys: KeyStore,
        makeSession: @escaping SessionMaker,
        readyLimit: TimeInterval,
        afterTake: @escaping () -> Void = {},
        clock: @escaping () -> TimeInterval = { ProcessInfo.processInfo.systemUptime }
    ) {
        self.pipe = pipe
        self.via = via
        self.owner = owner
        self.keys = keys
        self.makeSession = makeSession
        self.readyLimit = readyLimit
        self.afterTake = afterTake
        self.clock = clock
    }

    func run() {
        var ready = false
        Self.log.info("\(String(describing: self.via)) 접속 시작")
        do {
            try serve { ready = true }
            show(ready ? .lost(name: owner.lastName) : .waiting)
        } catch let error as CoreError {
            Self.log.warning("연결 오류: \(String(describing: error))")
            show(.failed(reason: Self.reason(error)))
        } catch is PipeError {
            show(ready ? .lost(name: owner.lastName) : .waiting)
        } catch let error as KeyError {
            show(.failed(reason: error.reason))
        } catch {
            // 무엇이든 화면에 알리고 이 접속만 닫는다.
            Self.log.error("예상 못 한 오류: \(String(describing: error))")
            show(.failed(reason: "연결 오류"))
        }
        pipe.close()
        if let mine { owner.release(mine) }
        Self.log.info("\(String(describing: self.via)) 접속 끝")
    }

    /// 이어받은 뒤에는 화면을 바꾸고, 그 전에는 아이폰 USB 의 실패 이유만 아무 연결도 없을 때 보인다.
    private func show(_ next: LinkState) {
        if let mine {
            owner.show(mine, next)
        } else if via == .usb, case .failed = next {
            owner.showIfIdle(next)
        }
    }

    private func alive() -> Bool {
        mine.map(owner.isCurrent) ?? true
    }

    /// 이어받기. 졌으면 false
    private func take(computer: Data?) -> Bool {
        let pipe = self.pipe
        let close: @Sendable () -> Void = { pipe.close() }
        let took: Int?
        switch via {
        case .usb:
            took = owner.takeUsb(close: close)
            if let took, let computer { owner.ready(took, computer: computer) }
        case .bluetooth:
            guard let computer else { return false }
            took = owner.takeBluetooth(computer: computer, close: close)
        }
        guard let took else {
            Self.log.info("\(String(describing: self.via)) 접속이 이어받지 못함")
            return false
        }
        mine = took
        afterTake()
        return true
    }

    private func serve(onReady: () -> Void) throws {
        let identity = try keys.identity()
        let started = clock()
        let session = try makeSession(identity, try keys.computers(), via == .usb)
        var heard = clock()
        var ready = false
        var computer = Data()
        var subscribed: Subscription?
        let location = LocationGate()
        var sentKills = Set<UInt64>()

        func flush() throws {
            let output = session.takeOutput()
            if !output.isEmpty { try pipe.send(output) }
        }

        // 새 연결마다 마지막 구독과 결과를 못 받은 종료 명령을 같은 번호로 다시 보낸다.
        func sendQueued() throws {
            let now = owner.subscription
            if now != subscribed {
                try session.subscribe(main: now.big, tiles: Array(now.small))
                subscribed = now
            }
            // 위치는 Ready 뒤 이 안에서만 보낸다. 모르면 보내지 않는다.
            switch location.take(owner.currentLocation) {
            case .at(let latitude, let longitude)?:
                try session.sendLocation(latitude: latitude, longitude: longitude)
            case .off?:
                try session.sendNoLocation()
            case .unknown?, nil:
                break
            }
            for kill in owner.pendingKills(computer: computer) where sentKills.insert(kill.commandId).inserted {
                try session.killProcess(requestId: kill.commandId, pid: kill.pid, startedAt: kill.startedAt)
            }
            try flush()
        }

        func handleEvents() throws -> Bool {
            while let event = session.nextEvent() {
                switch event {
                case .pairingCode(let code):
                    // 짝 맺기는 USB 로만 온다(블루투스 세션은 짝 맺기를 거절한다).
                    if mine == nil, via == .bluetooth || !take(computer: nil) { return false }
                    show(.pairing(code: code))
                case .paired(let key):
                    try keys.trust(key)
                case .ready(let key):
                    if let mine {
                        owner.ready(mine, computer: key)
                    } else if !take(computer: key) {
                        return false
                    }
                    ready = true
                    computer = key
                    onReady()
                    show(.connected(name: owner.lastName, via: via))
                case .computerName(let name):
                    owner.lastName = name
                    show(.connected(name: name, via: via))
                case .performance(let value):
                    if let mine { owner.receivedPerf(mine, value) }
                case .processes(let items):
                    if let mine { owner.receivedProcesses(mine, items) }
                case .weather(let value):
                    if let mine { owner.receivedWeather(mine, value) }
                case .commandResult(let id, let ok, let reason):
                    owner.receivedKill(KillResult(commandId: id, ok: ok, reason: reason))
                }
            }
            return true
        }

        try flush()
        while alive() {
            if mine == nil, clock() - started > readyLimit {
                Self.log.info("확인 시간이 지남")
                return
            }
            if mine != nil, let same = owner.takeAnswer() {
                do {
                    try session.confirm(same)
                } catch {
                    try? flush()
                    throw error
                }
                try flush()
                if try !handleEvents() { return }
            }
            if ready { try sendQueued() }
            guard let bytes = try pipe.receive() else { return }
            if !bytes.isEmpty {
                heard = clock()
                try session.feed(bytes)
                if mine == nil {
                    // 이어받을지 먼저 정한다. 질 연결이 응답을 보내 컴퓨터가 연결됨으로 알지 않게.
                    if try !handleEvents() { return }
                    try flush()
                } else {
                    try flush()
                    if try !handleEvents() { return }
                }
            } else if clock() - heard > Self.silence {
                return
            }
        }
    }

    static func reason(_ error: CoreError) -> String {
        switch error {
        case .version: "컴퓨터와 버전이 다릅니다. 두 앱을 최신 버전으로 업데이트하세요"
        case .notPaired: "등록되지 않은 컴퓨터입니다. 컴퓨터의 SideDesk 설정에서 이 기기를 등록하세요"
        case .rejected: "등록을 취소했습니다"
        case .broken(let reason): "연결 오류: \(reason)"
        }
    }
}

struct KeyError: Error, Equatable {
    let reason: String
}

/// 한 연결 안에서 위치를 언제 보낼지 정한다. 접속마다 새로 만든다.
/// 모르면 보내지 않고, 같은 상태는 한 번만 보낸다.
final class LocationGate {
    private var sent = LocationState.unknown

    /// 보낼 것이 있으면 그 상태, 없으면 nil
    func take(_ now: LocationState) -> LocationState? {
        if now == .unknown || now == sent { return nil }
        sent = now
        return now
    }
}
