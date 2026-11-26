import Foundation
import Observation

/// 단조 시각(밀리초). LinkOwner 가 곡을 받은 때를 적는 시계와 같다.
func monotonicMillis() -> Int64 {
    Int64(Foundation.ProcessInfo.processInfo.systemUptime * 1000)
}

/// 진행 위치. 재생 중이면 받은 때부터 흐르고, 끊긴 때나 전체 길이에서 멈춘다.
func positionAt(_ now: NowPlaying, _ nowMs: Int64, lostAt: Int64? = nil) -> Int64 {
    guard now.playing else { return now.positionMs }
    let until = lostAt.map { min($0, nowMs) } ?? nowMs
    let position = now.positionMs + max(0, until - now.receivedAt)
    return now.durationMs > 0 ? min(position, now.durationMs) : position
}

/// "1:24", 한 시간이 넘으면 "1:02:03"
func clockText(_ ms: Int64) -> String {
    let total = max(0, ms) / 1000
    let h = total / 3600
    let m = total % 3600 / 60
    let s = total % 60
    return h > 0 ? String(format: "%d:%02d:%02d", h, m, s) : String(format: "%d:%02d", m, s)
}

func playerName(_ player: MusicPlayer) -> String {
    player == .spotify ? "Spotify" : "음악"
}

/// 음악 큰 화면과 작은 칸(안드로이드 `MusicViewModel`)
@MainActor
@Observable
final class MusicModel {
    static let noticeFor: Duration = .seconds(4)
    nonisolated static let volumeGapMillis: Int64 = 100
    static let settle: Duration = .seconds(2)

    /// 실패할 때만 한 줄. 4초 뒤 사라진다
    private(set) var notice: String?
    /// 누르고 결과를 기다리는 동안 먼저 보이는 재생 여부
    private(set) var optimistic: Bool?
    /// 끄는 동안 손가락 볼륨
    private(set) var dragging: Int?
    /// 끊긴 때(단조 밀리초). 진행 바를 여기서 멈춘다
    private(set) var lostAt: Int64?

    @ObservationIgnored private let link: any ComputerLink
    @ObservationIgnored private let buzzer: any Buzzer
    @ObservationIgnored private let newCommandId: () -> UInt64
    @ObservationIgnored private let clock: () -> Int64
    @ObservationIgnored private let giveUpAfter: Duration
    @ObservationIgnored private var pending: [UInt64: MusicAction] = [:]
    @ObservationIgnored private var clearing: Task<Void, Never>?
    @ObservationIgnored private var settling: Task<Void, Never>?
    @ObservationIgnored private var lastVolumeSent = Int64.min / 2
    @ObservationIgnored private var volumeLater: Task<Void, Never>?
    @ObservationIgnored private var volumeFailed = false

    init(
        link: any ComputerLink,
        buzzer: any Buzzer,
        newCommandId: @escaping () -> UInt64,
        clock: @escaping () -> Int64 = monotonicMillis,
        giveUpAfter: Duration = .seconds(LinkOwner.killLimit)
    ) {
        self.link = link
        self.buzzer = buzzer
        self.newCommandId = newCommandId
        self.clock = clock
        self.giveUpAfter = giveUpAfter
        link.feed.onResult { [weak self] result in self?.onResult(result) }
        // 새 상태가 오면 먼저 보인 모양을 버리고 그것을 따른다.
        link.feed.onNowPlaying { [weak self] in
            self?.settling?.cancel()
            self?.optimistic = nil
        }
    }

    var now: NowPlaying? { link.feed.nowPlaying }

    var volume: Volume? { link.feed.volume }

    /// 지금 곡의 앨범 그림(jpeg). 없으면 기본 그림
    var art: Data? {
        guard let hash = link.feed.nowPlaying?.artwork, hash != 0 else { return nil }
        return link.feed.artworks[hash]
    }

    var dimmed: Bool {
        if case .connected = link.feed.state { false } else { true }
    }

    var hasTrack: Bool { (link.feed.nowPlaying?.player ?? .none) != .none }
    var playing: Bool { optimistic ?? (link.feed.nowPlaying?.playing ?? false) }
    var level: Int? { dragging ?? link.feed.volume?.level }

    /// 연결이 끊기거나 이어질 때 부른다. 끊긴 때 진행 바를 멈춘다.
    func connectionChanged(connected: Bool) {
        if connected {
            lostAt = nil
        } else if lostAt == nil {
            lostAt = clock()
        }
    }

    private var canAct: Bool { hasTrack && !dimmed }

    /// 버튼 모양을 먼저 바꾸고 보낸다. 실패하거나 2초 안에 새 상태가 안 오면 되돌린다.
    func playPause() {
        guard canAct, link.feed.nowPlaying?.canPlay == true else { return }
        let target = !playing
        guard send(.playPause) else { return }
        optimistic = target
    }

    func next() {
        if canAct, link.feed.nowPlaying?.canNext == true { _ = send(.next) }
    }

    func previous() {
        if canAct, link.feed.nowPlaying?.canPrevious == true { _ = send(.previous) }
    }

    /// 끄는 동안 0.1초에 한 번 마지막 값만 보낸다. 컴퓨터에서 온 값은 손을 뗄 때까지 무시한다.
    func dragVolume(_ level: Int) {
        guard let volume, volume.settable, !dimmed else { return }
        let value = max(0, min(100, level))
        if dragging == nil { volumeFailed = false }
        dragging = value
        let wait = Self.volumeGapMillis - (clock() - lastVolumeSent)
        volumeLater?.cancel()
        if wait <= 0 {
            sendVolume(value)
        } else {
            volumeLater = Task { [weak self] in
                try? await Task.sleep(for: .milliseconds(wait))
                guard !Task.isCancelled, let self, let value = dragging else { return }
                sendVolume(value)
            }
        }
    }

    func releaseVolume() {
        guard let value = dragging else { return }
        volumeLater?.cancel()
        sendVolume(value)
        dragging = nil
    }

    private func sendVolume(_ level: Int) {
        lastVolumeSent = clock()
        let command = MusicCommand(commandId: newCommandId(), action: .setVolume, level: level)
        if link.music(command) { pending[command.commandId] = .setVolume }
    }

    private func send(_ action: MusicAction) -> Bool {
        buzzer.tap()
        let command = MusicCommand(commandId: newCommandId(), action: action)
        guard link.music(command) else {
            show(LinkOwner.notConnected)
            return false
        }
        pending[command.commandId] = action
        let wait = giveUpAfter
        Task { [weak self] in
            try? await Task.sleep(for: wait)
            self?.link.giveUp(command.commandId)
        }
        return true
    }

    private func onResult(_ result: CommandResult) {
        guard let action = pending.removeValue(forKey: result.commandId) else { return }
        if action == .setVolume {
            if !result.ok, !volumeFailed {
                volumeFailed = true
                show(Self.failure(result))
            }
        } else if !result.ok {
            if action == .playPause { optimistic = nil }
            show(Self.failure(result))
        } else if action == .playPause {
            settling?.cancel()
            settling = Task { [weak self] in
                try? await Task.sleep(for: Self.settle)
                guard !Task.isCancelled else { return }
                self?.optimistic = nil
            }
        }
    }

    private func show(_ text: String) {
        notice = text
        clearing?.cancel()
        clearing = Task { [weak self] in
            try? await Task.sleep(for: Self.noticeFor)
            guard !Task.isCancelled, let self, notice == text else { return }
            notice = nil
        }
    }

    static func failure(_ result: CommandResult) -> String {
        let reason = result.reason.trimmingCharacters(in: .whitespaces)
        return reason.isEmpty ? "실행하지 못했습니다" : reason
    }
}
