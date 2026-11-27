import Foundation
import Testing

@testable import SideDesk

private func session(_ id: String, _ state: AiState, since: Int64) -> AiSession {
    AiSession(id: id, tool: .claudeCode, title: id, project: "p", state: state, since: since, open: true)
}

@MainActor
struct AiModelTests {
    let link = FakeLink()
    let buzzer = FakeBuzzer()

    @Test func sortsActiveFirstThenRecentAndCounts() {
        var time: Int64 = 100_000
        let ai = AiModel(link: link, buzzer: buzzer, now: { time })
        #expect(ai.busy == nil, "받은 적 없음")
        link.feed.sessions = [session("a", .done, since: 9), session("b", .working, since: 1), session("c", .waiting, since: 5), session("d", .unknown, since: 3)]
        #expect(ai.sessions.map(\.id) == ["c", "b", "a", "d"])
        #expect(ai.busy == 2)
        #expect(ai.waiting)
        time += 1
    }

    @Test func alertsBuzzOnceInTwoSecondsAndLateOnesStaySilent() {
        var time: Int64 = 100_000
        let ai = AiModel(link: link, buzzer: buzzer, now: { time })
        var heard: [(UInt64, Bool)] = []
        ai.onAlert { alert, late in heard.append((alert.id, late)) }
        link.feed.emit(AiAlert(id: 1, sessionId: "a", title: "t", kind: .waiting, at: time))
        link.feed.emit(AiAlert(id: 2, sessionId: "a", title: "t", kind: .done, at: time))
        #expect(buzzer.buzzes == [.waiting], "2초 안 거듭은 한 번만")
        time += 3_000
        link.feed.emit(AiAlert(id: 3, sessionId: "a", title: "t", kind: .done, at: time - 31_000))
        #expect(buzzer.buzzes == [.waiting], "30초 넘게 늦으면 울리지 않음")
        #expect(ai.alertLate)
        #expect(ai.alert?.id == 3)
        #expect(heard.map(\.1) == [false, false, true])
    }

    @Test func alertDisappearsAfterItsTime() async throws {
        let ai = AiModel(link: link, buzzer: buzzer, now: { 0 }, showFor: .milliseconds(20))
        var cleared = 0
        ai.onCleared { cleared += 1 }
        link.feed.emit(AiAlert(id: 1, sessionId: "a", title: "t", kind: .done, at: 0))
        #expect(ai.alert != nil)
        try await Task.sleep(for: .milliseconds(300))
        #expect(ai.alert == nil)
        #expect(cleared == 1)
    }

    @Test func forgeTabsAndText() {
        let ai = AiModel(link: link, buzzer: buzzer)
        link.feed.forge = Forge(github: ForgeCounts(state: .ok, reviews: 1, mine: 2, notices: 60), gitlab: ForgeCounts(state: .missing, reviews: 0, mine: 0, notices: 0))
        #expect(ai.counts?.reviews == 1)
        ai.select(.gitlab)
        #expect(ai.counts?.state == .missing)
        #expect(forgeProblem(.missing, tool: "glab") == "glab 가 설치되어 있지 않습니다")
        #expect(noticeText(60) == "50+")
        #expect(noticeText(7) == "7")
    }

    @Test func elapsedTexts() {
        #expect(elapsedText(now: 59_000, since: 0) == "지금")
        #expect(elapsedText(now: 12 * 60_000, since: 0) == "12분")
        #expect(elapsedText(now: 7 * 3_600_000 + 4 * 60_000, since: 0) == "7시간 4분")
        #expect(elapsedText(now: 40 * 3_600_000, since: 0) == "1일 16시간")
        #expect(elapsedText(now: 0, since: 5_000) == "지금", "앞선 시각은 0")
    }

    @Test func sessionCardsShrinkToFiftySixThenScroll() {
        #expect(sessionUnit(height: 1000, active: 1, rest: 2) == 76)
        #expect(sessionUnit(height: 100, active: 5, rest: 9) == 56)
    }
}

@MainActor
struct ShortcutModelTests {
    let link = FakeLink()
    let buzzer = FakeBuzzer()

    @Test func pressBuzzesAndSendsOnlyTheNumber() {
        let model = ShortcutModel(link: link, buzzer: buzzer, newCommandId: { 42 })
        link.feed.shortcuts = [Shortcut(id: 3, name: "터미널", icon: "terminal", color: "slate", toggle: .none, system: false)]
        model.press(9)
        #expect(buzzer.taps == 0, "없는 번호는 무시")
        model.press(3)
        #expect(buzzer.taps == 1)
        #expect(link.shortcutRuns == [RunShortcut(commandId: 42, shortcutId: 3)])
    }

    @Test func onlyFailuresAreShown() {
        let model = ShortcutModel(link: link, buzzer: buzzer, newCommandId: { 42 })
        link.feed.shortcuts = [Shortcut(id: 3, name: "터미널", icon: "terminal", color: "slate", toggle: .none, system: false)]
        model.press(3)
        link.feed.emit(CommandResult(commandId: 42, ok: true))
        #expect(model.notice == nil)
        model.press(3)
        link.feed.emit(CommandResult(commandId: 42, ok: false, reason: "찾지 못함"))
        #expect(model.notice == "터미널을 실행하지 못했습니다. 찾지 못함")
        link.accepts = false
        model.press(3)
        #expect(model.notice == LinkOwner.notConnected)
    }

    @Test func failureSentenceDoesNotRepeat() {
        #expect(ShortcutModel.failure("", CommandResult(commandId: 1, ok: false, reason: "실행하지 못했습니다")) == "단축키를 실행하지 못했습니다")
        #expect(ShortcutModel.failure("빌드", CommandResult(commandId: 1, ok: false)) == "빌드를 실행하지 못했습니다")
    }

    @Test func gridIsFourByThreeWideAndThreeByFourTall() {
        #expect(shortcutGrid(width: 600, height: 300, count: 5) == ShortcutGrid(columns: 4, rows: 3, cell: 92, scrolls: false))
        let tall = shortcutGrid(width: 300, height: 600, count: 14)
        #expect(tall.columns == 3)
        #expect(tall.rows == 5)
        #expect(tall.scrolls)
    }

    @Test func glyphColorsFollowTheBackground() {
        #expect(onColor("sand") != onColor("slate"), "노란 바탕은 어두운 그림")
        #expect(glyphNames.count == 14)
    }
}

@MainActor
struct MusicModelTests {
    let link = FakeLink()
    let buzzer = FakeBuzzer()

    func track(playing: Bool = true, receivedAt: Int64 = 1_000) -> NowPlaying {
        NowPlaying(player: .music, title: "곡", artist: "", album: "", durationMs: 10_000, artwork: 0, playing: playing, positionMs: 2_000, canPlay: true, canNext: true, canPrevious: false, receivedAt: receivedAt)
    }

    @Test func positionRunsFromReceiptAndStopsAtLossOrEnd() {
        let now = track()
        #expect(positionAt(now, 4_000) == 5_000)
        #expect(positionAt(now, 4_000, lostAt: 2_000) == 3_000)
        #expect(positionAt(now, 99_000) == 10_000)
        #expect(positionAt(track(playing: false), 99_000) == 2_000)
        #expect(clockText(84_000) == "1:24")
        #expect(clockText(3_723_000) == "1:02:03")
    }

    @Test func playPauseShowsFirstAndFallsBackOnFailure() {
        var id: UInt64 = 0
        let music = MusicModel(link: link, buzzer: buzzer, newCommandId: { id += 1; return id }, clock: { 0 })
        link.feed.state = .connected(name: nil, via: .usb)
        link.feed.receive(nowPlaying: track())
        music.playPause()
        #expect(!music.playing, "먼저 멈춤으로 보임")
        #expect(buzzer.taps == 1)
        link.feed.emit(CommandResult(commandId: 1, ok: false, reason: "막힘"))
        #expect(music.playing, "실패하면 되돌림")
        #expect(music.notice == "막힘")
        music.playPause()
        link.feed.receive(nowPlaying: track(playing: false, receivedAt: 2_000))
        #expect(!music.playing, "새 상태를 따름")
        music.previous()
        #expect(link.musicCommands.map(\.action) == [.playPause, .playPause], "이전 곡이 막혀 있으면 안 보냄")
    }

    @Test func volumeDragSendsAtMostEveryTenthOfASecond() async throws {
        var time: Int64 = 1_000
        var id: UInt64 = 0
        let music = MusicModel(link: link, buzzer: buzzer, newCommandId: { id += 1; return id }, clock: { time })
        link.feed.state = .connected(name: nil, via: .usb)
        link.feed.volume = Volume(available: true, level: 10, muted: false, settable: true)
        music.dragVolume(20)
        music.dragVolume(30)
        music.dragVolume(140)
        #expect(link.musicCommands.map(\.level) == [20], "0.1초 안의 값은 미룸")
        #expect(music.level == 100)
        time += 200
        music.releaseVolume()
        #expect(link.musicCommands.map(\.level) == [20, 100])
        #expect(music.dragging == nil)
    }

    @Test func negativeVolumeBecomesZero() {
        let music = MusicModel(link: link, buzzer: buzzer, newCommandId: { 1 }, clock: { 10_000 })
        link.feed.state = .connected(name: nil, via: .usb)
        link.feed.volume = Volume(available: true, level: 10, muted: false, settable: true)
        music.dragVolume(-5)
        #expect(music.level == 0)
        #expect(link.musicCommands.last?.level == 0)
    }

    @Test func lostConnectionFreezesTheBar() {
        let music = MusicModel(link: link, buzzer: buzzer, newCommandId: { 1 }, clock: { 5_000 })
        music.connectionChanged(connected: false)
        #expect(music.lostAt == 5_000)
        music.connectionChanged(connected: true)
        #expect(music.lostAt == nil)
    }
}

@MainActor
struct RestModelTests {
    @Test func restsAfterTheTimeAndWakesOnTouch() async throws {
        let settings = MemorySettings()
        let rest = RestModel(settings: settings)
        #expect(rest.restAfter == .five)
        #expect(rest.keepOn == .charging)
        rest.setVisible(true)
        #expect(rest.isWaiting)
        rest.forceRest()
        #expect(rest.resting)
        rest.touched()
        #expect(!rest.resting)
        #expect(rest.isWaiting, "깨면 다시 잰다")
    }

    @Test func holdsStopTheTimerAndPairingWakes() {
        let rest = RestModel(settings: MemorySettings())
        rest.setVisible(true)
        rest.hold(.picking, true)
        #expect(!rest.isWaiting)
        rest.hold(.picking, false)
        #expect(rest.isWaiting)
        rest.forceRest()
        rest.hold(.noFocus, true)
        #expect(rest.resting, "초점 없음은 깨우지 않음")
        rest.hold(.pairing, true)
        #expect(!rest.resting, "짝 맺기 숫자는 깨움")
        rest.forceRest()
        #expect(!rest.resting, "막는 까닭이 있으면 바로 쉬기도 안 됨")
    }

    @Test func alertsWakeOnlyWhenAskedAndNotWhenLate() {
        let rest = RestModel(settings: MemorySettings())
        let alert = AiAlert(id: 1, sessionId: "s", title: "검증", kind: .waiting, at: 0)
        rest.setVisible(true)
        rest.forceRest()
        rest.alerted(alert, late: false)
        #expect(rest.resting)
        #expect(rest.alert == alert, "깨우지 않으면 아래에 한 줄")
        rest.setWakeOnAlert(true)
        rest.alerted(alert, late: true)
        #expect(rest.resting, "늦은 알림은 깨우지 않음")
        rest.alerted(alert, late: false)
        #expect(!rest.resting)
    }

    @Test func settingsAreSaved() {
        let settings = MemorySettings()
        let rest = RestModel(settings: settings)
        rest.setKeepOn(.always)
        rest.setRestAfter(.never)
        rest.setVibration(.strong)
        rest.setWakeOnAlert(true)
        let again = RestModel(settings: settings)
        #expect(again.keepOn == .always)
        #expect(again.restAfter == .never)
        #expect(again.vibration == .strong)
        #expect(again.wakeOnAlert)
        again.setVisible(true)
        #expect(!again.isWaiting, "안 함이면 쉬지 않음")
    }

    @Test func keepOnRulesAndBattery() {
        #expect(keepScreenOn(.always, charging: false, percent: 50))
        #expect(!keepScreenOn(.always, charging: false, percent: 14))
        #expect(keepScreenOn(.always, charging: false, percent: 15), "15% 는 켜 둠")
        #expect(keepScreenOn(.always, charging: true, percent: 5))
        #expect(keepScreenOn(.always, charging: false, percent: nil), "모르면 100 으로 봄")
        #expect(!keepScreenOn(.charging, charging: false, percent: 100))
        #expect(!keepScreenOn(.never, charging: true, percent: 100))
        #expect(batteryPercent(-1) == nil)
        #expect(batteryPercent(0.42) == 42)
    }

    @Test func hapticPatternsFollowTheKindAndStrength() {
        #expect(hapticPulses(.waiting, .normal)?.count == 2)
        #expect(hapticPulses(.done, .normal)?.count == 1)
        #expect(hapticPulses(nil, .weak)?.first?.length == 0.02)
        #expect(hapticPulses(.done, .off) == nil)
        #expect(abs(Vibration.normal.intensity - 160 / 255) < 0.001)
        #expect(abs(Vibration.weak.intensity - 80 / 255) < 0.001)
        #expect(Vibration.strong.intensity == 1)
        #expect(hapticPulses(.waiting, .normal)?.last?.start == 0.18, "두 번째는 0.18초 뒤")
    }
}

@MainActor
struct RestingSubscriptionTests {
    @Test func restingAsksOnlyForWeatherAndAi() {
        let link = FakeLink()
        let desk = DeskModel(link: link, newCommandId: { 1 })
        desk.setVisible(true)
        desk.setResting(true)
        #expect(link.subscriptions.last == Subscription(big: .weather, small: [.ai]))
        desk.setResting(false)
        #expect(link.subscriptions.last?.small.count == 5)
    }
}
