import Foundation
import Observation

/// 격자 모양. 가로 판은 4열 3줄, 세로 판은 3열 4줄(시안 .fd). 넘치면 줄이 늘고 민다.
struct ShortcutGrid: Equatable {
    var columns: Int
    var rows: Int
    var cell: Double
    var scrolls: Bool
}

func shortcutGrid(width: Double, height: Double, count: Int, gap: Double = 12) -> ShortcutGrid {
    let wide = width >= height
    let columns = wide ? 4 : 3
    let baseRows = wide ? 3 : 4
    let cell = min((width - gap * Double(columns - 1)) / Double(columns), (height - gap * Double(baseRows - 1)) / Double(baseRows))
    let rows = max(baseRows, Int((Double(count) / Double(columns)).rounded(.up)))
    return ShortcutGrid(columns: columns, rows: rows, cell: max(0, cell), scrolls: rows > baseRows)
}

/// 단축키 큰 화면과 작은 칸(안드로이드 `ShortcutViewModel`)
@MainActor
@Observable
final class ShortcutModel {
    static let noticeFor: Duration = .seconds(4)
    private static let generic = "실행하지 못했습니다"

    /// 실패할 때만 한 줄. 4초 뒤 사라진다
    private(set) var notice: String?

    @ObservationIgnored private let link: any ComputerLink
    @ObservationIgnored private let buzzer: any Buzzer
    @ObservationIgnored private let newCommandId: () -> UInt64
    @ObservationIgnored private let giveUpAfter: Duration
    @ObservationIgnored private var names: [UInt64: String] = [:]
    @ObservationIgnored private var clearing: Task<Void, Never>?

    init(link: any ComputerLink, buzzer: any Buzzer, newCommandId: @escaping () -> UInt64, giveUpAfter: Duration = .seconds(LinkOwner.killLimit)) {
        self.link = link
        self.buzzer = buzzer
        self.newCommandId = newCommandId
        self.giveUpAfter = giveUpAfter
        link.feed.onResult { [weak self] result in
            guard let self, let name = names.removeValue(forKey: result.commandId) else { return }
            if !result.ok { show(Self.failure(name, result)) }
        }
    }

    /// 지금 컴퓨터의 목록. 받은 적 없으면 nil
    var items: [Shortcut]? { link.feed.shortcuts }

    var dimmed: Bool {
        if case .connected = link.feed.state { false } else { true }
    }

    /// 진동 한 번 하고 번호만 보낸다. 성공은 말없이 넘어가고 실패만 알린다.
    func press(_ id: UInt32) {
        guard let item = items?.first(where: { $0.id == id }) else { return }
        buzzer.tap()
        let command = RunShortcut(commandId: newCommandId(), shortcutId: id)
        guard link.runShortcut(command) else {
            show(LinkOwner.notConnected)
            return
        }
        names[command.commandId] = item.name
        let wait = giveUpAfter
        Task { [weak self] in
            try? await Task.sleep(for: wait)
            self?.link.giveUp(command.commandId)
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

    /// 이름이 비면 "단축키를", 이유가 비거나 "실행하지 못했습니다"면 겹치지 않게 뺀다.
    static func failure(_ name: String, _ result: CommandResult) -> String {
        let blank = name.trimmingCharacters(in: .whitespaces).isEmpty
        let head = "\(blank ? "단축키를" : eul(name)) 실행하지 못했습니다"
        let reason = result.reason.trimmingCharacters(in: .whitespaces)
        return reason.isEmpty || reason == generic ? head : "\(head). \(reason)"
    }
}
