import Foundation

/// 메인 스레드에서 차례대로 돌린다(Task 와 달리 보낸 순서가 지켜진다).
func onMain(_ work: @escaping @MainActor @Sendable () -> Void) {
    DispatchQueue.main.async {
        MainActor.assumeIsolated(work)
    }
}

/// 컴퓨터가 접속해 오는 길 둘(USB, 블루투스)을 앱이 앞에 있을 때만 연다.
/// 어느 연결이 화면을 쓸지는 `LinkOwner` 가 정한다.
@MainActor
final class PhoneLink: ComputerLink {
    let feed: LinkFeed
    private let owner: LinkOwner
    private let usb: UsbListener
    private let bluetooth: BleLink

    init(keys: KeyStore = KeychainKeys(), makeSession: @escaping SessionMaker = Core.makeSession) {
        let feed = LinkFeed()
        self.feed = feed
        let owner = LinkOwner(
            publish: { snapshot in
                onMain {
                    feed.state = snapshot.state
                    if let perf = snapshot.perf, perf != feed.perf || feed.perf == nil {
                        feed.receive(perf: perf)
                    }
                    feed.processes = snapshot.processes
                    feed.weather = snapshot.weather
                }
            },
            emitKill: { result in
                onMain { feed.emit(result) }
            }
        )
        self.owner = owner
        usb = UsbListener(owner: owner, keys: keys, makeSession: makeSession, port: Core.usbPort)
        bluetooth = BleLink(owner: owner, keys: keys, makeSession: makeSession) { problem, decided in
            onMain {
                feed.bluetoothProblem = problem
                if decided { feed.bluetoothDecided = true }
            }
        }
    }

    func start() {
        owner.pause(false)
        usb.start()
        bluetooth.start()
    }

    func stop() {
        owner.pause(true)
        bluetooth.stop()
        usb.stop()
        owner.closeAll()
    }

    func subscribe(_ subscription: Subscription) {
        owner.subscription = subscription
    }

    func setLocation(_ location: LocationState) {
        owner.setLocation(location)
    }

    func kill(_ command: KillCommand) {
        owner.kill(command)
    }

    func giveUp(_ commandId: UInt64) {
        owner.giveUp(commandId)
    }

    func answer(_ same: Bool) {
        owner.setAnswer(same)
    }
}
