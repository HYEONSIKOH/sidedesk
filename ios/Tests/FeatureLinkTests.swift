import Foundation
import Testing

@testable import SideDesk

private func shortcut(_ id: UInt32) -> Shortcut {
    Shortcut(id: id, name: "키\(id)", icon: "terminal", color: "slate", toggle: .none, system: false)
}

private func track(playing: Bool = true) -> NowPlaying {
    NowPlaying(player: .music, title: "곡", artist: "", album: "", durationMs: 100_000, artwork: 7, playing: playing, positionMs: 1_000, canPlay: true, canNext: true, canPrevious: true)
}

/// 알림을 모은다.
final class Alerts: @unchecked Sendable {
    private let lock = NSLock()
    private var items: [AiAlert] = []

    var all: [AiAlert] {
        lock.lock()
        defer { lock.unlock() }
        return items
    }

    var emit: @Sendable (AiAlert) -> Void {
        { [self] alert in
            lock.lock()
            items.append(alert)
            lock.unlock()
        }
    }
}

struct FeatureOwnerTests {
    let clock = Clock()
    let results = Results()
    let alerts = Alerts()

    func owner() -> LinkOwner {
        let clock = clock
        return LinkOwner(clock: { clock.now }, emitResult: results.emit, emitAlert: alerts.emit)
    }

    /// 이 컴퓨터로 붙은 연결 하나
    func connected(_ owner: LinkOwner, _ computer: Data = mac) -> Int {
        let mine = owner.takeUsb(close: {})!
        owner.ready(mine, computer: computer)
        return mine
    }

    @Test func shortcutsRunOnlyOnTheComputerThatSentThemWithItsBook() {
        let owner = owner()
        let mine = connected(owner)
        #expect(!owner.runShortcut(RunShortcut(commandId: 1, shortcutId: 1)), "목록 전에는 안 보냄")
        owner.receivedShortcuts(mine, book: 9, [shortcut(1)])
        #expect(!owner.runShortcut(RunShortcut(commandId: 2, shortcutId: 5)), "목록에 없는 번호")
        #expect(owner.runShortcut(RunShortcut(commandId: 3, shortcutId: 1)))
        #expect(owner.pendingCommands(computer: mac) == [.shortcut(RunShortcut(commandId: 3, shortcutId: 1, book: 9))])
    }

    @Test func anotherComputerOverUsbOrBluetoothClearsShortcutsAndMusic() {
        let owner = owner()
        let mine = connected(owner)
        owner.receivedShortcuts(mine, book: 1, [shortcut(1)])
        owner.receivedNowPlaying(mine, track())
        owner.receivedVolume(mine, Volume(available: true, level: 5, muted: false, settable: true))
        _ = connected(owner, otherMac)
        #expect(owner.shortcuts == nil, "USB Ready 로 다른 컴퓨터")
        #expect(owner.nowPlaying == nil)
        #expect(owner.volume == nil)

        let again = owner.takeBluetooth(computer: otherMac, close: {})
        #expect(again == nil, "USB 가 살아 있으면 블루투스는 짐")
        let other = self.owner()
        let first = connected(other)
        other.receivedShortcuts(first, book: 1, [shortcut(1)])
        other.release(first)
        _ = other.takeBluetooth(computer: otherMac, close: {})
        #expect(other.shortcuts == nil, "블루투스로 다른 컴퓨터")
    }

    @Test func musicGoesOnlyToTheComputerOfTheTrack() {
        let owner = owner()
        let mine = connected(owner)
        #expect(!owner.music(MusicCommand(commandId: 1, action: .next)), "곡을 받기 전")
        owner.receivedNowPlaying(mine, track())
        #expect(owner.nowPlaying?.receivedAt == Int64(clock.now * 1000), "받은 때를 적음")
        #expect(owner.music(MusicCommand(commandId: 2, action: .next)))
        #expect(owner.pendingCommands(computer: mac) == [.music(MusicCommand(commandId: 2, action: .next))])
    }

    @Test func volumeKeepsOnlyTheLastValueForThisConnectionOnce() {
        let owner = owner()
        let mine = connected(owner)
        owner.receivedVolume(mine, Volume(available: true, level: 5, muted: false, settable: true))
        #expect(owner.music(MusicCommand(commandId: 1, action: .setVolume, level: 10)))
        #expect(owner.music(MusicCommand(commandId: 2, action: .setVolume, level: 20)))
        #expect(owner.pendingCommands(computer: mac).isEmpty, "볼륨은 대기열 밖")
        #expect(owner.takeVolume(mine)?.level == 20)
        #expect(owner.takeVolume(mine) == nil, "한 번만")
        #expect(owner.music(MusicCommand(commandId: 3, action: .setVolume, level: 30)))
        let next = connected(owner)
        #expect(owner.takeVolume(next) == nil, "다른 연결에서 든 값은 버림")
        owner.receivedResult(CommandResult(commandId: 1, ok: false, reason: "안 됨"))
        #expect(results.all == [CommandResult(commandId: 1, ok: false, reason: "안 됨")], "볼륨 실패는 넘김")
    }

    @Test func alertsComeOnceAndRecountOnANewConnection() {
        let owner = owner()
        let first = connected(owner)
        let alert = AiAlert(id: 1, sessionId: "s", title: "t", kind: .done, at: 0)
        owner.receivedAlert(first, alert)
        owner.receivedAlert(first, alert)
        #expect(alerts.all.count == 1)
        let second = connected(owner)
        owner.receivedAlert(first, alert)
        #expect(alerts.all.count == 1, "옛 연결의 알림은 버림")
        owner.receivedAlert(second, alert)
        #expect(alerts.all.count == 2, "컴퓨터가 다시 켜지면 번호가 1부터")
    }

    @Test func keepsTheLastEightArtworks() {
        let owner = owner()
        let mine = connected(owner)
        for hash in 1 ... 10 {
            owner.receivedArtwork(mine, hash: UInt64(hash), jpeg: Data([UInt8(hash)]))
        }
        owner.receivedArtwork(mine, hash: 0, jpeg: Data([0]))
        #expect(owner.artworks.count == LinkOwner.artworkLimit)
        #expect(owner.artworks[1] == nil)
        #expect(owner.artworks[10] == Data([10]))
        #expect(owner.artworks[0] == nil)
    }

    @Test func sessionsAndForgeOnlyFromTheCurrentConnection() {
        let owner = owner()
        let old = connected(owner)
        let now = connected(owner)
        let session = AiSession(id: "a", tool: .codex, title: "t", project: "p", state: .working, since: 0, open: true)
        owner.receivedSessions(old, [session])
        #expect(owner.sessions == nil)
        owner.receivedSessions(now, [session])
        owner.receivedForge(now, Forge(github: .unknown, gitlab: .unknown))
        #expect(owner.sessions?.count == 1)
        #expect(owner.forge != nil)
    }
}

struct FeatureConnectionTests {
    let clock = Clock()

    func run(_ owner: LinkOwner, _ session: FakeSession) {
        let clock = clock
        Connection(
            pipe: FakePipe([chunk], clock: clock), via: .usb, owner: owner, keys: MemoryKeys(computers: [mac]),
            makeSession: { _, _, _ in session }, readyLimit: 5, clock: { clock.now }
        ).run()
    }

    @Test func newEventsArriveOnlyAfterTakingOver() {
        let owner = LinkOwner(clock: { [clock] in clock.now })
        let session = FakeSession([.success([
            .shortcuts(book: 4, items: [shortcut(2)]),
            .ready(computerKey: mac),
            .shortcuts(book: 5, items: [shortcut(3)]),
            .nowPlaying(track()),
            .aiSessions([AiSession(id: "a", tool: .codex, title: "t", project: "p", state: .waiting, since: 0, open: true)]),
        ])])
        run(owner, session)
        #expect(owner.shortcuts?.map(\.id) == [3], "이어받기 전 목록은 버림")
        #expect(owner.nowPlaying?.title == "곡")
        #expect(owner.sessions?.first?.state == .waiting)
    }

    @Test func queuedCommandsAreResentByKindOnTheNextConnection() {
        let owner = LinkOwner(clock: { [clock] in clock.now })
        run(owner, FakeSession([.success([.ready(computerKey: mac)])]))
        owner.send(.shortcut(RunShortcut(commandId: 5, shortcutId: 2, book: 4)))
        owner.send(.music(MusicCommand(commandId: 6, action: .playPause)))
        owner.send(.kill(KillCommand(commandId: 7, pid: 1, startedAt: 1)))
        let session = FakeSession([.success([.ready(computerKey: mac)])])
        run(owner, session)
        #expect(session.shortcuts.map(\.0) == [5])
        #expect(session.shortcuts.first?.2 == 4, "장부 번호를 돌려줌")
        #expect(session.music.map(\.0) == [6])
        #expect(session.music.first?.1 == .playPause)
        #expect(session.killed == [7])
    }
}
