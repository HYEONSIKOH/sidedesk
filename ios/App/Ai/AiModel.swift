import Foundation
import Observation

enum ForgeTab: String, CaseIterable, Sendable {
    case github = "GitHub"
    case gitlab = "GitLab"
}

/// 컴퓨터는 알림을 50 개까지만 센다. 50 이면 그보다 많을 수 있다.
let noticeCap = 50

func noticeText(_ count: Int) -> String {
    count >= noticeCap ? "\(noticeCap)+" : "\(count)"
}

/// 지난 시간 글자. 1분 안은 "지금", 그다음 "12분", "7시간 4분", "1일 16시간"
func elapsedText(now: Int64, since: Int64) -> String {
    let minutes = max(0, now - since) / 60_000
    let hours = minutes / 60
    let days = hours / 24
    if minutes < 1 { return "지금" }
    if hours < 1 { return "\(minutes)분" }
    if days < 1 { return minutes % 60 == 0 ? "\(hours)시간" : "\(hours)시간 \(minutes % 60)분" }
    return hours % 24 == 0 ? "\(days)일" : "\(days)일 \(hours % 24)시간"
}

func stateText(_ state: AiState) -> String {
    switch state {
    case .working: "작업 중"
    case .waiting: "답변 대기"
    case .done: "끝남"
    case .unknown: "알 수 없음"
    }
}

func alertText(_ kind: AlertKind) -> String {
    kind == .waiting ? "답변 대기" : "작업 끝남"
}

/// AI 세션 큰 화면과 작은 칸, 알림(안드로이드 `AiViewModel`)
@MainActor
@Observable
final class AiModel {
    static let showFor: Duration = .seconds(4)
    nonisolated static let lateMillis: Int64 = 30_000
    nonisolated static let quietMillis: Int64 = 2_000

    private(set) var tab = ForgeTab.github
    /// 지금 띄운 알림. 4초 뒤 사라진다
    private(set) var alert: AiAlert?
    /// 지금 알림이 30초 넘게 늦게 왔는지. 늦은 알림은 울리지도 깨우지도 않는다.
    private(set) var alertLate = false

    @ObservationIgnored private let link: any ComputerLink
    @ObservationIgnored private let buzzer: any Buzzer
    @ObservationIgnored private let now: () -> Int64
    @ObservationIgnored private var lastBuzz = Int64.min / 2
    @ObservationIgnored private var clearing: Task<Void, Never>?
    @ObservationIgnored private var alertListeners: [(AiAlert, Bool) -> Void] = []
    @ObservationIgnored private var clearedListeners: [() -> Void] = []

    init(link: any ComputerLink, buzzer: any Buzzer, now: @escaping () -> Int64 = { Int64(Date().timeIntervalSince1970 * 1000) }) {
        self.link = link
        self.buzzer = buzzer
        self.now = now
        // 화면과 상관없이 처음부터 듣는다. 알림은 다시 보내 주지 않으므로 늦게 들으면 사라진다.
        link.feed.onAlert { [weak self] alert in self?.show(alert) }
    }

    /// 작업 중과 답변 대기 먼저, 그다음 나머지. 같은 무리 안에서는 최근 것 먼저
    var sessions: [AiSession] {
        (link.feed.sessions ?? []).sorted { a, b in
            if a.active != b.active { return a.active }
            return a.since > b.since
        }
    }

    /// 작은 칸 숫자(작업 중 + 답변 대기). 받은 적 없으면 nil
    var busy: Int? { link.feed.sessions.map { $0.filter(\.active).count } }

    var waiting: Bool { link.feed.sessions?.contains { $0.state == .waiting } ?? false }

    var counts: ForgeCounts? {
        guard let forge = link.feed.forge else { return nil }
        return tab == .github ? forge.github : forge.gitlab
    }

    var dimmed: Bool {
        if case .connected = link.feed.state { false } else { true }
    }

    func select(_ tab: ForgeTab) {
        self.tab = tab
    }

    /// 쉬는 화면이 알림을 듣는다(늦었는지 함께).
    func onAlert(_ listener: @escaping (AiAlert, Bool) -> Void) {
        alertListeners.append(listener)
    }

    /// 알림이 사라질 때(쉬는 화면의 줄도 지운다)
    func onCleared(_ listener: @escaping () -> Void) {
        clearedListeners.append(listener)
    }

    /// 화면은 넘기지 않는다. 4초 동안 보이고, 늦게 온 알림은 깨우지 않는다.
    private func show(_ alert: AiAlert) {
        let time = now()
        let late = time - alert.at > Self.lateMillis
        if !late, time - lastBuzz >= Self.quietMillis {
            buzzer.buzz(alert.kind)
            lastBuzz = time
        }
        self.alert = alert
        alertLate = late
        for listener in alertListeners {
            listener(alert, late)
        }
        clearing?.cancel()
        clearing = Task { [weak self] in
            try? await Task.sleep(for: Self.showFor)
            guard !Task.isCancelled, let self, self.alert == alert else { return }
            self.alert = nil
            for listener in clearedListeners {
                listener()
            }
        }
    }
}
