import Foundation
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
    /// AI 세션 목록. 받은 적 없으면 nil
    var sessions: [AiSession]?
    var forge: Forge?
    /// 지금 붙은 컴퓨터의 단축키 목록. 다른 컴퓨터가 붙으면 지운다.
    var shortcuts: [Shortcut]?
    /// 지금 재생 중인 곡. 다른 컴퓨터가 붙으면 지운다.
    var nowPlaying: NowPlaying?
    var volume: Volume?
    /// 해시로 기억한 앨범 그림(jpeg)
    var artworks: [UInt64: Data] = [:]
    /// 블루투스 상태를 알게 됐다(권한 창이 닫혔다). 위치 안내는 그 뒤에 띄운다.
    var bluetoothDecided = false

    /// 마지막으로 넣은 연결 값의 번호(PhoneLink 가 쓴다)
    @ObservationIgnored var version = 0
    @ObservationIgnored private var perfListeners: [(Perf) -> Void] = []
    @ObservationIgnored private var resultListeners: [(CommandResult) -> Void] = []
    @ObservationIgnored private var alertListeners: [(AiAlert) -> Void] = []
    @ObservationIgnored private var nowPlayingListeners: [() -> Void] = []

    init() {}

    /// 새 성능 값이 올 때마다(같은 값이어도) 부른다. 그래프가 쓴다.
    func onPerf(_ listener: @escaping (Perf) -> Void) {
        perfListeners.append(listener)
    }

    func onResult(_ listener: @escaping (CommandResult) -> Void) {
        resultListeners.append(listener)
    }

    /// 곡이나 재생 상태가 바뀌면 부른다.
    func onNowPlaying(_ listener: @escaping () -> Void) {
        nowPlayingListeners.append(listener)
    }

    func receive(nowPlaying value: NowPlaying?) {
        guard value != nowPlaying else { return }
        nowPlaying = value
        for listener in nowPlayingListeners {
            listener()
        }
    }

    /// 세션 알림. 버리지 않는다. 한 연결 안의 같은 번호는 한 번만 온다.
    func onAlert(_ listener: @escaping (AiAlert) -> Void) {
        alertListeners.append(listener)
    }

    func emit(_ alert: AiAlert) {
        for listener in alertListeners {
            listener(alert)
        }
    }

    func receive(perf value: Perf) {
        perf = value
        for listener in perfListeners {
            listener(value)
        }
    }

    func emit(_ result: CommandResult) {
        for listener in resultListeners {
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
    func send(_ command: Command)
    /// 지금 컴퓨터의 목록에 있는 단축키면 명령으로 두고 true, 아니면 보내지 않고 false
    func runShortcut(_ command: RunShortcut) -> Bool
    /// 보이는 곡이 지금 컴퓨터 것이면 명령으로 두고 true, 아니면 보내지 않고 false
    func music(_ command: MusicCommand) -> Bool
    /// 기한 안에 결과가 없으면 부른다. 아직 남아 있으면 지우고 실패로 알린다.
    func giveUp(_ commandId: UInt64)
    /// 등록 번호를 확인한 결과
    func answer(_ same: Bool)
}
