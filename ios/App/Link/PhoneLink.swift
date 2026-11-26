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
                    // 잠금 밖에서 발행하므로 순서가 뒤바뀔 수 있다. 더 새 값만 넣는다.
                    guard snapshot.version > feed.version else { return }
                    feed.version = snapshot.version
                    // 같은 값을 다시 넣으면 Observation 이 보는 화면을 다시 그리므로 바뀐 것만 넣는다.
                    if feed.state != snapshot.state { feed.state = snapshot.state }
                    if let perf = snapshot.perf, perf != feed.perf || feed.perf == nil {
                        feed.receive(perf: perf)
                    }
                    if feed.processes != snapshot.processes { feed.processes = snapshot.processes }
                    if feed.weather != snapshot.weather { feed.weather = snapshot.weather }
                    if feed.sessions != snapshot.sessions { feed.sessions = snapshot.sessions }
                    if feed.forge != snapshot.forge { feed.forge = snapshot.forge }
                    if feed.shortcuts != snapshot.shortcuts { feed.shortcuts = snapshot.shortcuts }
                    feed.receive(nowPlaying: snapshot.nowPlaying)
                    if feed.volume != snapshot.volume { feed.volume = snapshot.volume }
                    if feed.artworks.keys != snapshot.artworks.keys { feed.artworks = snapshot.artworks }
                }
            },
            emitResult: { result in
                onMain { feed.emit(result) }
            },
            emitAlert: { alert in
                onMain { feed.emit(alert) }
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

    func send(_ command: Command) {
        owner.send(command)
    }

    func runShortcut(_ command: RunShortcut) -> Bool {
        owner.runShortcut(command)
    }

    func music(_ command: MusicCommand) -> Bool {
        owner.music(command)
    }

    func giveUp(_ commandId: UInt64) {
        owner.giveUp(commandId)
    }

    func answer(_ same: Bool) {
        owner.setAnswer(same)
    }
}
