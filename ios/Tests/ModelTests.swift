import Foundation
import Testing

@testable import SideDesk

@MainActor
struct ClockModelTests {
    let link = FakeLink()
    var nowMillis: Int64 = 10_000_000

    func clock(_ settings: MemorySettings = MemorySettings()) -> ClockModel {
        let now = nowMillis
        return ClockModel(link: link, settings: settings, now: { Date(timeIntervalSince1970: Double(now) / 1000) })
    }

    @Test func startsWithTheSavedFaceAndTheme() {
        let clock = clock(MemorySettings(face: "Dial", theme: "Light"))
        #expect(clock.face == .dial)
        #expect(clock.theme == .light)
    }

    @Test func startsWithDefaultsForUnknownValues() {
        let clock = clock(MemorySettings(face: "Moon", theme: "Blue"))
        #expect(clock.face == .digits)
        #expect(clock.theme == .dark)
    }

    @Test func swipingChangesTheFaceOnlyWhilePicking() {
        let settings = MemorySettings()
        let clock = clock(settings)
        clock.nextFace()
        #expect(clock.face == .digits)
        clock.pick()
        clock.nextFace()
        #expect(clock.face == .dial)
        clock.previousFace()
        clock.previousFace()
        #expect(clock.face == .overview)
        #expect(settings.face == "Overview", "다시 켜도 남는다")
        clock.done()
        clock.nextFace()
        #expect(clock.face == .overview)
    }

    @Test func savesTheTheme() {
        let settings = MemorySettings()
        let clock = clock(settings)
        clock.setTheme(.light)
        #expect(settings.theme == "Light")
        #expect(clock.theme == .light)
    }

    @Test func showsTheWayItIsConnected() {
        let clock = clock()
        #expect(clock.via == nil)
        link.feed.state = .connected(name: nil, via: .bluetooth)
        #expect(clock.via == .bluetooth)
        link.feed.state = .lost(name: nil)
        #expect(clock.via == nil)
    }

    @Test func noLocationShowsDashesAndNoCredit() {
        let clock = clock()
        link.feed.state = .connected(name: nil, via: .usb)
        #expect(clock.weather == nil)
        #expect(clock.temperature == "--°")
        #expect(clock.credit == nil)
        #expect(!clock.weatherDimmed)
    }

    @Test func dimsTheWeatherWhenLost() {
        let clock = clock()
        link.feed.weather = weather(fetchedAt: nowMillis)
        link.feed.state = .connected(name: nil, via: .usb)
        #expect(!clock.weatherDimmed)
        #expect(clock.temperature == "3°")
        link.feed.state = .lost(name: nil)
        #expect(clock.weatherDimmed)
    }

    @Test func dimsWeatherOlderThanAnHourButNotFromTheFuture() {
        let clock = clock()
        link.feed.state = .connected(name: nil, via: .usb)
        link.feed.weather = weather(fetchedAt: nowMillis - ClockModel.staleMillis - 1)
        #expect(clock.weatherDimmed)
        link.feed.weather = weather(fetchedAt: nowMillis + 60_000)
        #expect(!clock.weatherDimmed)
    }

    @Test func creditsWhereTheWeatherCameFrom() {
        let clock = clock()
        link.feed.weather = weather(source: .kma)
        #expect(clock.credit == "날씨 자료 기상청")
        #expect(clock.shortCredit == "기상청")
        link.feed.weather = weather(source: .openMeteo)
        #expect(clock.credit == "날씨 자료 Open-Meteo.com")
        #expect(clock.shortCredit == "Open-Meteo")
        link.feed.weather = weather(source: .unknown)
        #expect(clock.credit == nil)
    }

    @Test func stopsCheckingWhileHidden() {
        let clock = clock()
        clock.setVisible(true)
        #expect(clock.isTicking)
        clock.setVisible(false)
        #expect(!clock.isTicking)
    }
}

@MainActor
struct DeskModelTests {
    let link = FakeLink()
    var ids: [UInt64] = [11, 12, 13]

    func desk(giveUpAfter: Duration = .seconds(30)) -> DeskModel {
        var ids = ids
        return DeskModel(link: link, newCommandId: { ids.removeFirst() }, giveUpAfter: giveUpAfter)
    }

    @Test func startsAtHome() {
        let desk = desk()
        #expect(desk.big == .home)
        #expect(desk.small == [.ai, .perf, .processes, .shortcuts, .music])
    }

    @Test func showsTheLatestPerfAndKeepsTheLastMinuteOfNetwork() {
        let desk = desk()
        for i in 0 ..< 70 {
            link.feed.receive(perf: perf(cpu: Float(i), down: Int64(i), up: Int64(i * 2)))
        }
        #expect(desk.perf?.cpu == 69)
        #expect(desk.received.count == DeskModel.history)
        #expect(desk.received.first == 10)
        #expect(desk.sent.last == 138)
    }

    @Test func sortsProcessesByCpuThenMemory() {
        let desk = desk()
        link.feed.processes = [process(1, "a", cpu: 1, memory: 30), process(2, "b", cpu: 5, memory: 10)]
        #expect(desk.rows.map(\.name) == ["b", "a"])
        desk.sortBy(.memory)
        #expect(desk.rows.map(\.name) == ["a", "b"])
    }

    @Test func subscribesToWhatIsVisibleAndClearsWhenHidden() {
        let desk = desk()
        desk.show(.perf)
        #expect(link.subscriptions.allSatisfy { $0 == .none }, "보이기 전에는 빈 구독만 보낸다")
        desk.setVisible(true)
        #expect(link.subscriptions.last == Subscription(big: .perf, small: [.weather, .ai, .processes, .shortcuts, .music]))
        desk.setVisible(false)
        #expect(link.subscriptions.last == Subscription.none)
    }

    @Test func swipingGoesAroundTheScreens() {
        let desk = desk()
        desk.next()
        #expect(desk.big == .ai)
        desk.next()
        #expect(desk.big == .perf)
        for _ in 0 ..< 4 { desk.next() }
        #expect(desk.big == .home)
        desk.previous()
        #expect(desk.big == .music)
    }

    @Test func tappingAProcessAsksWithTheSharedConfirm() {
        let desk = desk()
        desk.show(.processes)
        let target = process(4, "Xcode", cpu: 12.5, memory: 2_000_000_000)
        desk.ask(target)
        #expect(desk.asking == target)
        let confirm = DeskView.killConfirm(target)
        #expect(confirm.title == "Xcode을(를) 끝낼까요?")
        #expect(confirm.danger)
        #expect(confirm.confirmLabel == "끝내기")
        #expect(confirm.description.contains("12.5%"))
        #expect(confirm.glyph == "X")
        desk.next()
        #expect(desk.big == .processes, "묻는 중에는 화면이 안 바뀐다")
    }

    @Test func cancelingSendsNothing() {
        let desk = desk()
        desk.ask(process(4, "빌드"))
        desk.cancel()
        #expect(desk.asking == nil)
        #expect(link.kills.isEmpty)
        desk.confirmKill()
        #expect(link.kills.isEmpty, "묻지 않았으면 아무것도 안 함")
    }

    @Test func confirmingSendsTheKillCommandAndShowsTheResult() {
        let desk = desk()
        desk.ask(process(4, "빌드"))
        desk.confirmKill()
        #expect(link.kills == [KillCommand(commandId: 11, pid: 4, startedAt: 100)])
        #expect(desk.notice == "빌드를 끝내는 중")
        link.feed.emit(CommandResult(commandId: 99, ok: true))
        #expect(desk.notice == "빌드를 끝내는 중", "다른 명령 결과는 무시")
        link.feed.emit(CommandResult(commandId: 11, ok: false, reason: "권한이 없습니다"))
        #expect(desk.notice == "빌드를 끝내지 못했습니다. 권한이 없습니다")
    }

    @Test func givesUpAfterTheLimit() async throws {
        let desk = desk(giveUpAfter: .milliseconds(10))
        desk.ask(process(4, "빌드"))
        desk.confirmKill()
        try await Task.sleep(for: .milliseconds(200))
        #expect(link.givenUp == [11])
    }

    @Test func picksTheParticleByTheLastLetter() {
        #expect(eul("빌드") == "빌드를")
        #expect(eul("터미널") == "터미널을")
        #expect(eul("Xcode") == "Xcode을(를)")
        #expect(eul("") == "을(를)")
    }

    @Test func dimsAndKeepsTheLastValuesWhenLost() {
        let desk = desk()
        link.feed.state = .connected(name: nil, via: .usb)
        link.feed.receive(perf: perf(cpu: 40))
        #expect(!desk.dimmed)
        link.feed.state = .lost(name: nil)
        #expect(desk.dimmed)
        #expect(desk.perf?.cpu == 40)
    }
}

@MainActor
struct HomeModelTests {
    let link = FakeLink()

    @Test func startsByWaiting() {
        let home = HomeModel(link: link)
        #expect(home.computerName == nil)
        #expect(home.problem == nil)
        #expect(home.pairingCode == nil)
    }

    @Test func showsTheLatestName() {
        let home = HomeModel(link: link)
        link.feed.state = .connected(name: "a", via: .usb)
        link.feed.state = .connected(name: "b", via: .usb)
        #expect(home.computerName == "b")
    }

    @Test func showsAFailureAndRecovers() {
        let home = HomeModel(link: link)
        link.feed.state = .failed(reason: "열지 못함")
        #expect(home.problem == "열지 못함")
        link.feed.state = .waiting
        #expect(home.problem == nil)
    }

    @Test func showsTheCodeAndPassesTheAnswer() {
        let home = HomeModel(link: link)
        link.feed.state = .pairing(code: "123456")
        #expect(home.pairingCode == "123456")
        home.answer(true)
        #expect(link.answers == [true])
    }

    @Test func dimsTheLastNameWhenLost() {
        let home = HomeModel(link: link)
        link.feed.state = .lost(name: "맥")
        #expect(home.computerName == "맥")
        #expect(home.dimmed)
    }

    @Test func showsTheBluetoothNotice() {
        let home = HomeModel(link: link)
        link.feed.bluetoothProblem = "블루투스가 꺼져 있어 USB 로만 연결합니다"
        #expect(home.notice == "블루투스가 꺼져 있어 USB 로만 연결합니다")
    }
}

struct OverviewGridTests {
    @Test func picksTheLeastStretchedLayout() {
        #expect(overviewGrid(width: 800, height: 200, gap: 10) == (4, 1))
        #expect(overviewGrid(width: 400, height: 400, gap: 10) == (2, 2))
        #expect(overviewGrid(width: 150, height: 800, gap: 10) == (1, 4))
    }
}

struct KeysTests {
    @Test func splitsKeysAndIgnoresAPartialTail() {
        let bytes = Data(repeating: 1, count: 32) + Data(repeating: 2, count: 32) + Data([3, 3])
        let keys = KeychainKeys.split(bytes)
        #expect(keys == [Data(repeating: 1, count: 32), Data(repeating: 2, count: 32)])
        #expect(KeychainKeys.split(Data()).isEmpty)
    }

    @Test func keychainKeepsTheIdentityAndTrustsOnce() throws {
        let keys = KeychainKeys(service: "io.github.hyeonsikoh.sidedesk.test.\(UUID().uuidString)")
        let first = try keys.identity()
        #expect(first.privateKey.count == 32)
        #expect(first.publicKey.count == 32)
        #expect(try keys.identity() == first)
        try keys.trust(mac)
        try keys.trust(mac)
        try keys.trust(otherMac)
        #expect(try keys.computers() == [mac, otherMac])
    }
}

@MainActor
final class FakeLocation: LocationPermission {
    var undecided = true
    var asked = 0
    var refreshed = 0
    var stopped = 0

    func ask() { asked += 1 }
    func refresh() { refreshed += 1 }
    func stop() { stopped += 1 }
}

@MainActor
struct LocationNoticeTests {
    let link = FakeLink()
    let settings = MemorySettings()
    let location = FakeLocation()

    func world() -> World {
        World(link: link, settings: settings, location: location)
    }

    @Test func tappingOutsideClosesOnlyThisTime() {
        let world = world()
        world.askingLocation = true
        world.answerLocation(nil)
        #expect(!world.askingLocation)
        #expect(!settings.askedLocation, "다음에 켤 때 다시 묻는다")
        #expect(location.asked == 0)
        #expect(link.locations.isEmpty)
    }

    @Test func notUsingSendsNoLocationAndRemembers() {
        let world = world()
        world.answerLocation(false)
        #expect(settings.askedLocation)
        #expect(link.locations == [.off])
        #expect(location.asked == 0)
    }

    @Test func nextAsksTheSystemAndRemembers() {
        let world = world()
        world.answerLocation(true)
        #expect(settings.askedLocation)
        #expect(location.asked == 1)
        #expect(link.locations.isEmpty)
    }

    @Test func foregroundStartsAndBackgroundStops() {
        let world = world()
        world.foreground()
        #expect(link.started == 1)
        #expect(location.refreshed == 1)
        world.clock.pick()
        world.background()
        #expect(link.stopped == 1)
        #expect(location.stopped == 1)
        #expect(!world.clock.picking, "뒤로 가면 고르기를 닫는다")
        #expect(link.subscriptions.last == Subscription.none)
    }
}

@MainActor
struct SettingsTests {
    @Test func savedValuesComeBackAfterARestart() throws {
        let name = "sidedesk.test.\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: name))
        defer { defaults.removePersistentDomain(forName: name) }
        let first = DefaultsSettings(defaults: defaults)
        first.face = "Calendar"
        first.theme = "Light"
        first.askedLocation = true
        let again = ClockModel(link: FakeLink(), settings: DefaultsSettings(defaults: try #require(UserDefaults(suiteName: name))))
        #expect(again.face == .calendar)
        #expect(again.theme == .light)
        #expect(DefaultsSettings(defaults: defaults).askedLocation)
    }
}

struct SmallRulesTests {
    @Test func swipesNeedSixtyPointsSideways() {
        #expect(swipeStep(CGSize(width: -61, height: 10)) == 1)
        #expect(swipeStep(CGSize(width: 61, height: -10)) == -1)
        #expect(swipeStep(CGSize(width: -59, height: 0)) == 0)
        #expect(swipeStep(CGSize(width: -80, height: 120)) == 0, "세로가 더 크면 스크롤")
    }

    @Test func strangeTemperaturesDoNotCrash() {
        #expect(degrees(3.6) == "4°")
        #expect(degrees(-0.4) == "0°")
        #expect(degrees(.nan) == "--°")
        #expect(degrees(.infinity) == "--°")
    }

    @Test func datesUseTheGregorianCalendar() {
        let time = ClockTime(Date(timeIntervalSince1970: 1_795_000_000))
        #expect(ClockTime.calendar.identifier == .gregorian)
        #expect((1 ... 12).contains(time.month))
        #expect(time.daysInMonth >= 28)
    }
}

@MainActor
struct StaleBoundaryTests {
    @Test func exactlyOneHourIsStillFresh() {
        let link = FakeLink()
        let now: Int64 = 50_000_000
        let clock = ClockModel(link: link, settings: MemorySettings(), now: { Date(timeIntervalSince1970: Double(now) / 1000) })
        link.feed.state = .connected(name: nil, via: .usb)
        link.feed.weather = weather(fetchedAt: now - ClockModel.staleMillis)
        #expect(!clock.weatherDimmed)
        link.feed.weather = weather(fetchedAt: now - ClockModel.staleMillis - 1)
        #expect(clock.weatherDimmed)
    }
}
