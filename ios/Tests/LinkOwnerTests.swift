import Foundation
import Testing

@testable import SideDesk

/// 닫기를 센다.
final class Closes: @unchecked Sendable {
    private let lock = NSLock()
    private var count = 0

    var value: Int {
        lock.lock()
        defer { lock.unlock() }
        return count
    }

    var close: @Sendable () -> Void {
        { [self] in
            lock.lock()
            count += 1
            lock.unlock()
        }
    }
}

final class Results: @unchecked Sendable {
    private let lock = NSLock()
    private var items: [KillResult] = []

    var all: [KillResult] {
        lock.lock()
        defer { lock.unlock() }
        return items
    }

    var emit: @Sendable (KillResult) -> Void {
        { [self] result in
            lock.lock()
            items.append(result)
            lock.unlock()
        }
    }
}

struct LinkOwnerTests {
    let clock = Clock()
    let results = Results()

    func owner() -> LinkOwner {
        let clock = clock
        return LinkOwner(clock: { clock.now }, emitKill: results.emit)
    }

    @Test func usbTakesOverAndClosesBluetooth() {
        let owner = owner()
        let air = Closes()
        let bluetooth = owner.takeBluetooth(computer: mac, close: air.close)!
        let usb = owner.takeUsb(close: {})!
        #expect(air.value == 1)
        #expect(!owner.isCurrent(bluetooth))
        #expect(owner.isCurrent(usb))
    }

    @Test func bluetoothLosesToALiveUsb() {
        let owner = owner()
        _ = owner.takeUsb(close: {})
        #expect(owner.takeBluetooth(computer: mac, close: {}) == nil)
    }

    @Test func bluetoothLosesToAnotherComputerButReplacesTheSameOne() {
        let owner = owner()
        let old = Closes()
        _ = owner.takeBluetooth(computer: mac, close: old.close)
        #expect(owner.takeBluetooth(computer: otherMac, close: {}) == nil)
        #expect(owner.takeBluetooth(computer: mac, close: {}) != nil)
        #expect(old.value == 1)
    }

    @Test func nothingTakesOverWhilePaused() {
        let owner = owner()
        owner.pause(true)
        #expect(owner.takeUsb(close: {}) == nil)
        #expect(owner.takeBluetooth(computer: mac, close: {}) == nil)
        owner.pause(false)
        #expect(owner.takeUsb(close: {}) != nil)
    }

    @Test func onlyTheCurrentConnectionChangesTheScreen() {
        let owner = owner()
        let first = owner.takeUsb(close: {})!
        let second = owner.takeUsb(close: {})!
        owner.show(first, .waiting)
        owner.show(second, .connected(name: "맥", via: .usb))
        owner.show(first, .lost(name: "맥"))
        #expect(owner.state == .connected(name: "맥", via: .usb))
    }

    @Test func showsAnIdleFailureOnlyWithoutAConnection() {
        let owner = owner()
        owner.showIfIdle(.failed(reason: "등록되지 않은 컴퓨터"))
        #expect(owner.state == .failed(reason: "등록되지 않은 컴퓨터"))
        let mine = owner.takeUsb(close: {})!
        owner.show(mine, .connected(name: nil, via: .usb))
        owner.showIfIdle(.failed(reason: "다른 앱"))
        #expect(owner.state == .connected(name: nil, via: .usb))
    }

    @Test func keepsACommandUntilItsResultComes() {
        let owner = owner()
        let mine = owner.takeUsb(close: {})!
        owner.ready(mine, computer: mac)
        owner.kill(KillCommand(commandId: 5, pid: 1, startedAt: 2))
        #expect(owner.pendingKills(computer: mac).map(\.commandId) == [5])
        owner.receivedKill(KillResult(commandId: 5, ok: true))
        #expect(owner.pendingKills(computer: mac).isEmpty)
        #expect(results.all == [KillResult(commandId: 5, ok: true)])
    }

    @Test func dropsResultsOfUnknownCommands() {
        let owner = owner()
        owner.receivedKill(KillResult(commandId: 9, ok: true))
        #expect(results.all.isEmpty)
    }

    @Test func sendsOnlyToTheComputerItWasChosenForAndKeepsItAfterTheEnd() {
        let owner = owner()
        let mine = owner.takeBluetooth(computer: mac, close: {})!
        owner.release(mine)
        owner.kill(KillCommand(commandId: 1, pid: 1, startedAt: 1))
        #expect(owner.pendingKills(computer: otherMac).isEmpty)
        #expect(owner.pendingKills(computer: mac).count == 1)
    }

    @Test func stopsSendingAfterTheLimit() {
        let owner = owner()
        _ = owner.takeBluetooth(computer: mac, close: {})
        owner.kill(KillCommand(commandId: 1, pid: 1, startedAt: 1))
        clock.advance(LinkOwner.killLimit + 1)
        #expect(owner.pendingKills(computer: mac).isEmpty)
    }

    @Test func givingUpRemovesAndReportsOnce() {
        let owner = owner()
        _ = owner.takeBluetooth(computer: mac, close: {})
        owner.kill(KillCommand(commandId: 1, pid: 1, startedAt: 1))
        owner.giveUp(1)
        owner.giveUp(1)
        owner.receivedKill(KillResult(commandId: 1, ok: true))
        #expect(results.all == [KillResult(commandId: 1, ok: false, reason: LinkOwner.notConnected)])
    }

    @Test func failsAtOnceWithoutAnyComputer() {
        let owner = owner()
        owner.kill(KillCommand(commandId: 3, pid: 1, startedAt: 1))
        #expect(results.all == [KillResult(commandId: 3, ok: false, reason: LinkOwner.notConnected)])
    }

    @Test func takesValuesOnlyFromTheCurrentConnection() {
        let owner = owner()
        let old = owner.takeUsb(close: {})!
        let now = owner.takeUsb(close: {})!
        owner.receivedWeather(old, weather(fetchedAt: 1))
        owner.receivedPerf(old, perf(cpu: 1))
        #expect(owner.weather == nil)
        #expect(owner.perf == nil)
        owner.receivedWeather(now, weather(fetchedAt: 2))
        owner.receivedProcesses(now, [process(1, "a")])
        #expect(owner.weather?.fetchedAt == 2)
        #expect(owner.processes?.count == 1)
    }

    @Test func forgetsTheWeatherWhenLocationIsTurnedOff() {
        let owner = owner()
        #expect(owner.currentLocation == .unknown)
        let mine = owner.takeUsb(close: {})!
        owner.receivedWeather(mine, weather())
        owner.setLocation(.off)
        #expect(owner.weather == nil)
        owner.receivedWeather(mine, weather())
        #expect(owner.weather == nil)
    }

    @Test func answerIsTakenOnceAndClearedByANewConnection() {
        let owner = owner()
        owner.setAnswer(true)
        #expect(owner.takeAnswer() == true)
        #expect(owner.takeAnswer() == nil)
        owner.setAnswer(false)
        _ = owner.takeUsb(close: {})
        #expect(owner.takeAnswer() == nil)
    }
}

struct CheckingSlotTests {
    @Test func newUsbReplacesTheOneBeingChecked() {
        let clock = Clock()
        let slot = CheckingSlot()
        let first = FakePipe([], clock: clock)
        let second = FakePipe([], clock: clock)
        slot.replace(with: first)
        slot.replace(with: second)
        #expect(first.isClosed)
        #expect(!second.isClosed)
        slot.release(first)
        #expect(slot.isHeld, "옛 접속은 새 자리를 놓지 못한다")
        slot.release(second)
        #expect(!slot.isHeld)
    }

    @Test func goingBackgroundClosesTheOneBeingChecked() {
        let slot = CheckingSlot()
        let pipe = FakePipe([], clock: Clock())
        slot.replace(with: pipe)
        slot.clear()
        #expect(pipe.isClosed)
        #expect(!slot.isHeld)
    }
}

struct LocationGateTests {
    @Test func sendsOnlyKnownChanges() {
        let gate = LocationGate()
        #expect(gate.take(.unknown) == nil)
        #expect(gate.take(.off) == .off)
        #expect(gate.take(.off) == nil)
        #expect(gate.take(.at(latitude: 1, longitude: 2)) == .at(latitude: 1, longitude: 2))
        #expect(gate.take(.at(latitude: 1, longitude: 2)) == nil)
    }
}
