import Foundation
import Observation

enum Sort: Sendable {
    case cpu
    case memory
}

/// 큰 화면에 띄울 수 있는 것. 옆으로 밀면 이 차례로 돈다.
enum Screen: CaseIterable, Sendable {
    case home
    case ai
    case perf
    case processes
    case shortcuts
    case music

    var topic: Topic {
        switch self {
        case .home: .weather
        case .ai: .ai
        case .perf: .perf
        case .processes: .processes
        case .shortcuts: .shortcuts
        case .music: .music
        }
    }
}

/// 이름 뒤 조사. 한글로 끝나면 받침에 맞추고, 아니면 둘 다 적는다.
func eul(_ name: String) -> String {
    guard let last = name.unicodeScalars.last, (0xAC00 ... 0xD7A3).contains(last.value) else {
        return name + "을(를)"
    }
    return name + ((last.value - 0xAC00) % 28 != 0 ? "을" : "를")
}

/// 큰 화면과 작은 칸, 성능, 프로세스(안드로이드 `DeskViewModel`)
@MainActor
@Observable
final class DeskModel {
    nonisolated static let history = 60

    private(set) var big = Screen.home
    /// 오래된 것부터. 그래프용
    private(set) var received: [Int64] = []
    private(set) var sent: [Int64] = []
    private(set) var sort = Sort.cpu
    /// 끝낼지 묻는 중인 프로세스
    private(set) var asking: ComputerProcess?
    var notice: String?

    @ObservationIgnored private let link: any ComputerLink
    /// 명령 번호. 앱을 다시 켜도 겹치지 않아야 한다.
    @ObservationIgnored private let newCommandId: () -> UInt64
    @ObservationIgnored private let giveUpAfter: Duration
    @ObservationIgnored private var visible = false
    @ObservationIgnored private var names: [UInt64: String] = [:]

    init(link: any ComputerLink, newCommandId: @escaping () -> UInt64, giveUpAfter: Duration = .seconds(LinkOwner.killLimit)) {
        self.link = link
        self.newCommandId = newCommandId
        self.giveUpAfter = giveUpAfter
        link.feed.onPerf { [weak self] perf in
            guard let self else { return }
            received = Array((received + [perf.receivePerSecond]).suffix(Self.history))
            sent = Array((sent + [perf.sendPerSecond]).suffix(Self.history))
        }
        link.feed.onResult { [weak self] result in
            guard let self, let name = names.removeValue(forKey: result.commandId) else { return }
            notice = Self.message(name, result)
        }
    }

    var perf: Perf? { link.feed.perf }

    var rows: [ComputerProcess] {
        let all = link.feed.processes ?? []
        switch sort {
        case .cpu: return all.sorted { $0.cpu > $1.cpu }
        case .memory: return all.sorted { $0.memoryBytes > $1.memoryBytes }
        }
    }

    /// 끊겨서 마지막 값을 흐리게 보인다
    var dimmed: Bool {
        if case .connected = link.feed.state { false } else { true }
    }

    /// 오른쪽 작은 칸. 큰 화면에 띄운 것은 뺀다.
    var small: [Screen] { Screen.allCases.filter { $0 != big } }

    /// 화면이 보이는지. 안 보이면 구독을 비운다.
    func setVisible(_ value: Bool) {
        visible = value
        tellComputer()
    }

    /// 프로세스 끝내기를 묻는 중에는 화면을 바꾸지 않는다.
    func show(_ screen: Screen) {
        guard asking == nil else { return }
        big = screen
        tellComputer()
    }

    func next() {
        let all = Screen.allCases
        show(all[((all.firstIndex(of: big) ?? 0) + 1) % all.count])
    }

    func previous() {
        let all = Screen.allCases
        show(all[((all.firstIndex(of: big) ?? 0) + all.count - 1) % all.count])
    }

    func sortBy(_ sort: Sort) {
        self.sort = sort
    }

    func ask(_ process: ComputerProcess) {
        asking = process
    }

    func cancel() {
        asking = nil
    }

    func confirmKill() {
        guard let target = asking else { return }
        let id = newCommandId()
        names[id] = target.name
        link.send(.kill(KillCommand(commandId: id, pid: target.pid, startedAt: target.startedAt)))
        let wait = giveUpAfter
        Task { [weak self] in
            try? await Task.sleep(for: wait)
            self?.link.giveUp(id)
        }
        asking = nil
        notice = "\(eul(target.name)) 끝내는 중"
    }

    func dismissNotice() {
        notice = nil
    }

    @ObservationIgnored private var resting = false

    /// 쉬는 동안은 시계와 AI 알림만 쓰므로 날씨와 AI 만 받는다. 깨면 원래 표로 돌아간다.
    func setResting(_ value: Bool) {
        guard resting != value else { return }
        resting = value
        tellComputer()
    }

    private func tellComputer() {
        let subscription: Subscription = if !visible {
            .none
        } else if resting {
            Subscription(big: .weather, small: [.ai])
        } else {
            Subscription(big: big.topic, small: Set(small.map(\.topic)))
        }
        link.subscribe(subscription)
    }

    private static func message(_ name: String, _ result: CommandResult) -> String {
        if result.ok { return "\(eul(name)) 끝냈습니다" }
        if result.reason.trimmingCharacters(in: .whitespaces).isEmpty { return "\(eul(name)) 끝내지 못했습니다" }
        return "\(eul(name)) 끝내지 못했습니다. \(result.reason)"
    }
}
