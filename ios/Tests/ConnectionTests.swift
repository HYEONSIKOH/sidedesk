import Foundation
import Testing

@testable import SideDesk

/// 화면으로 보낸 상태를 차례로 적는다. 숫자가 뜨면 답을 넣을 수도 있다.
final class States: @unchecked Sendable {
    private let lock = NSLock()
    private var items: [LinkState] = []
    weak var owner: LinkOwner?
    var answerPairing: Bool?

    var all: [LinkState] {
        lock.lock()
        defer { lock.unlock() }
        return items
    }

    var publish: @Sendable (LinkSnapshot) -> Void {
        { [self] snapshot in
            lock.lock()
            items.append(snapshot.state)
            let answer = answerPairing
            lock.unlock()
            if case .pairing = snapshot.state, let answer { owner?.setAnswer(answer) }
        }
    }
}

extension FakeSession: @unchecked Sendable {}

struct ConnectionTests {
    let clock = Clock()
    let states = States()
    let keys = MemoryKeys(computers: [mac])

    func owner() -> LinkOwner {
        let clock = clock
        let owner = LinkOwner(clock: { clock.now }, publish: states.publish)
        states.owner = owner
        return owner
    }

    /// 연결 하나를 끝까지 돌린다. 세션을 만들 때 받은 짝 맺기 허용 값을 돌려준다.
    @discardableResult
    func run(_ via: Via, _ session: FakeSession, _ pipe: FakePipe, _ owner: LinkOwner, limit: TimeInterval = 5) -> Bool {
        let clock = clock
        let pairing = AllowPairing()
        Connection(
            pipe: pipe,
            via: via,
            owner: owner,
            keys: keys,
            makeSession: { _, _, allow in
                pairing.set(allow)
                return session
            },
            readyLimit: limit,
            clock: { clock.now }
        ).run()
        return pairing.value
    }

    /// 블루투스로 붙어 화면을 쓰고 있는 맥 하나
    func connectedByBluetooth(_ owner: LinkOwner, _ closes: Closes) {
        let mine = owner.takeBluetooth(computer: mac, close: closes.close)!
        owner.show(mine, .connected(name: "맥", via: .bluetooth))
    }

    @Test func usbBeforeCheckingChangesNothingAndClosesAfterTheLimit() {
        let owner = owner()
        let air = Closes()
        connectedByBluetooth(owner, air)
        let pipe = FakePipe([chunk], clock: clock, endWhenEmpty: false)
        run(.usb, FakeSession([.success([])]), pipe, owner)
        #expect(owner.state == .connected(name: "맥", via: .bluetooth))
        #expect(air.value == 0)
        #expect(pipe.isClosed)
    }

    @Test func pairingCodeTakesOverUsbAndTheAnswerGoesToTheSession() throws {
        let owner = owner()
        let air = Closes()
        connectedByBluetooth(owner, air)
        states.answerPairing = true
        let session = FakeSession([
            .success([.pairingCode("123456")]),
            .success([.paired(computerKey: otherMac), .ready(computerKey: otherMac)]),
        ])
        let allowed = run(.usb, session, FakePipe([chunk, chunk], clock: clock), owner)
        #expect(allowed, "USB 는 짝 맺기를 받는다")
        #expect(air.value == 1, "USB 가 블루투스를 이긴다")
        #expect(states.all.contains(.pairing(code: "123456")))
        #expect(session.confirmed == [true])
        #expect(try keys.computers().contains(otherMac))
        #expect(states.all.contains(.connected(name: nil, via: .usb)))
    }

    @Test func readyConnectsAndAnEndDimsTheLastName() {
        let owner = owner()
        let session = FakeSession([.success([.ready(computerKey: mac), .computerName("내 맥")])])
        run(.usb, session, FakePipe([chunk], clock: clock), owner)
        #expect(states.all.contains(.connected(name: "내 맥", via: .usb)))
        #expect(owner.state == .lost(name: "내 맥"))
    }

    @Test func bluetoothNeverPairs() {
        let owner = owner()
        let session = FakeSession([.success([.pairingCode("123456")])])
        let allowed = run(.bluetooth, session, FakePipe([chunk], clock: clock), owner, limit: 2)
        #expect(!allowed)
        #expect(owner.state == .waiting)
        #expect(states.all.isEmpty)
    }

    @Test func aNewConnectionResendsSubscriptionLocationAndPendingKills() {
        let owner = owner()
        owner.subscription = Subscription(big: .perf, small: [.weather])
        owner.setLocation(.at(latitude: 37.5, longitude: 127))
        run(.usb, FakeSession([.success([.ready(computerKey: mac)])]), FakePipe([chunk], clock: clock), owner)
        owner.send(.kill(KillCommand(commandId: 42, pid: 7, startedAt: 1)))
        let session = FakeSession([.success([.ready(computerKey: mac)])])
        run(.bluetooth, session, FakePipe([chunk], clock: clock), owner)
        #expect(session.killed == [42])
        #expect(session.subscribed.count == 1)
        #expect(session.subscribed.first?.0 == .perf)
        #expect(session.located == ["37.5,127.0"])
    }

    @Test func silenceEndsTheConnection() {
        let owner = owner()
        let pipe = FakePipe([chunk], clock: clock, endWhenEmpty: false)
        run(.usb, FakeSession([.success([.ready(computerKey: mac)])]), pipe, owner)
        #expect(owner.state == .lost(name: nil))
        #expect(pipe.isClosed)
    }

    @Test func aRefusedUsbShowsItsReasonOnlyWhenNothingElseIsConnected() {
        let owner = owner()
        run(.usb, FakeSession([.failure(.notPaired)]), FakePipe([chunk], clock: clock), owner)
        #expect(owner.state == .failed(reason: Connection.reason(.notPaired)))

        let busy = self.owner()
        connectedByBluetooth(busy, Closes())
        run(.usb, FakeSession([.failure(.notPaired)]), FakePipe([chunk], clock: clock), busy)
        #expect(busy.state == .connected(name: "맥", via: .bluetooth))
    }

    @Test func backgroundWhileCheckingUsbDoesNotTakeOver() {
        let owner = owner()
        owner.pause(true)
        let pipe = FakePipe([chunk], clock: clock)
        run(.usb, FakeSession([.success([.ready(computerKey: mac)])]), pipe, owner)
        #expect(owner.state == .waiting)
        #expect(pipe.isClosed)
    }

    @Test func valuesReachTheOwnerOnlyAfterTakingOver() {
        let owner = owner()
        let session = FakeSession([
            .success([.performance(perf(cpu: 3)), .ready(computerKey: mac), .performance(perf(cpu: 9))]),
        ])
        run(.bluetooth, session, FakePipe([chunk], clock: clock), owner)
        #expect(owner.perf?.cpu == 9)
    }
}

final class AllowPairing: @unchecked Sendable {
    private let lock = NSLock()
    private var allowed = false

    var value: Bool {
        lock.lock()
        defer { lock.unlock() }
        return allowed
    }

    func set(_ value: Bool) {
        lock.lock()
        allowed = value
        lock.unlock()
    }
}
