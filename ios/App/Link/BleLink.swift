import CoreBluetooth
import Foundation

/// 블루투스 LE 로 자기를 알리고, 맥이 L2CAP 채널로 붙어 오면 받는다(안드로이드 `BleLink` 와 같은 규칙).
/// 아이폰은 알림 신호에 서비스 데이터를 못 실으므로 PSM 과 알림 신호 지문을 읽기 전용 GATT 특성에 둔다.
/// 블루투스로는 등록하지 않고, 키를 확인하기 전에는 지금 연결과 화면을 건드리지 않는다.
///
/// 관리자와 그 콜백은 전용 직렬 큐에서만 돈다. 상태는 그 큐에서만 바꾼다.
final class BleLink: NSObject, CBPeripheralManagerDelegate, @unchecked Sendable {
    /// 확인 전 접속을 닫는 시간
    static let readyLimit: TimeInterval = 2

    private let owner: LinkOwner
    private let keys: KeyStore
    private let makeSession: SessionMaker
    /// 블루투스를 쓸 수 없는 이유(nil 이면 문제 없음)와, 상태를 알게 됐는지
    private let report: @Sendable (_ problem: String?, _ decided: Bool) -> Void
    private let queue = DispatchQueue(label: "io.github.hyeonsikoh.sidedesk.ble")

    private var manager: CBPeripheralManager?
    private var running = false
    private var publishing = false
    private var psm: CBL2CAPPSM?
    private var service: CBMutableService?
    /// Ready 전 접속은 하나만 받는다.
    private let checking = NSLock()
    private var checkingTaken = false

    init(
        owner: LinkOwner,
        keys: KeyStore,
        makeSession: @escaping SessionMaker,
        report: @escaping @Sendable (_ problem: String?, _ decided: Bool) -> Void
    ) {
        self.owner = owner
        self.keys = keys
        self.makeSession = makeSession
        self.report = report
    }

    func start() {
        queue.async { [self] in
            running = true
            // 처음 만들 때 권한을 묻는다.
            if let manager {
                if manager.state == .poweredOn { open() }
            } else {
                manager = CBPeripheralManager(delegate: self, queue: queue)
            }
        }
    }

    func stop() {
        queue.async { [self] in
            running = false
            close()
        }
    }

    private func open() {
        guard running, let manager, manager.state == .poweredOn, psm == nil, !publishing else { return }
        publishing = true
        manager.publishL2CAPChannel(withEncryption: false)
    }

    private func close() {
        guard let manager else { return }
        manager.stopAdvertising()
        manager.removeAllServices()
        service = nil
        if let psm { manager.unpublishL2CAPChannel(psm) }
        psm = nil
    }

    private func say(_ problem: String?) {
        report(problem, true)
    }

    func peripheralManagerDidUpdateState(_ peripheral: CBPeripheralManager) {
        switch peripheral.state {
        case .poweredOn:
            say(nil)
            open()
        case .poweredOff:
            psm = nil
            publishing = false
            service = nil
            say("블루투스가 꺼져 있어 USB 로만 연결합니다")
        case .unauthorized:
            say("블루투스 권한이 없어 USB 로만 연결합니다")
        case .unsupported:
            say("이 기기는 블루투스를 쓸 수 없어 USB 로만 연결합니다")
        default:
            break
        }
    }

    func peripheralManager(_ peripheral: CBPeripheralManager, didPublishL2CAPChannel PSM: CBL2CAPPSM, error: Error?) {
        publishing = false
        if error != nil {
            say("블루투스 연결을 열지 못해 USB 로만 연결합니다")
            return
        }
        guard running else {
            peripheral.unpublishL2CAPChannel(PSM)
            return
        }
        psm = PSM
        do {
            let value = try Core.serviceData(psm: PSM, publicKey: keys.identity().publicKey)
            let info = CBMutableCharacteristic(
                type: CBUUID(string: Core.infoUuid),
                properties: .read,
                value: value,
                permissions: .readable
            )
            let service = CBMutableService(type: CBUUID(string: Core.serviceUuid), primary: true)
            service.characteristics = [info]
            self.service = service
            peripheral.add(service)
        } catch {
            say("블루투스 연결을 열지 못해 USB 로만 연결합니다")
        }
    }

    func peripheralManager(_ peripheral: CBPeripheralManager, didAdd service: CBService, error: Error?) {
        guard error == nil, running, service === self.service else {
            if error != nil { say("블루투스 연결을 열지 못해 USB 로만 연결합니다") }
            return
        }
        peripheral.startAdvertising([
            CBAdvertisementDataServiceUUIDsKey: [CBUUID(string: Core.serviceUuid)],
            CBAdvertisementDataLocalNameKey: "SideDesk",
        ])
    }

    func peripheralManagerDidStartAdvertising(_ peripheral: CBPeripheralManager, error: Error?) {
        say(error == nil ? nil : "블루투스 알림 신호를 내지 못해 USB 로만 연결합니다")
    }

    func peripheralManager(_ peripheral: CBPeripheralManager, didOpen channel: CBL2CAPChannel?, error: Error?) {
        guard let channel, error == nil else { return }
        let pipe = ChannelPipe(channel)
        guard running, takeChecking() else {
            pipe.close()
            return
        }
        let thread = Thread { [self] in
            // 이 접속이 잡은 확인 자리는 이 접속이 한 번만 놓는다.
            let held = OnceFlag()
            let letGo = { [self] in if held.take() { releaseChecking() } }
            Connection(
                pipe: pipe,
                via: .bluetooth,
                owner: owner,
                keys: keys,
                makeSession: makeSession,
                readyLimit: Self.readyLimit,
                afterTake: letGo
            ).run()
            letGo()
        }
        thread.name = "sidedesk-ble"
        thread.start()
    }

    private func takeChecking() -> Bool {
        checking.lock()
        defer { checking.unlock() }
        if checkingTaken { return false }
        checkingTaken = true
        return true
    }

    private func releaseChecking() {
        checking.lock()
        checkingTaken = false
        checking.unlock()
    }
}

/// 한 번만 true
final class OnceFlag: @unchecked Sendable {
    private let lock = NSLock()
    private var done = false

    func take() -> Bool {
        lock.lock()
        defer { lock.unlock() }
        if done { return false }
        done = true
        return true
    }
}

/// L2CAP 채널. 맥 `BleConn` 처럼 실행 루프 없이 스트림을 짧게 들여다본다.
final class ChannelPipe: LinkPipe, @unchecked Sendable {
    static let readWait: TimeInterval = 0.25
    static let readPoll: TimeInterval = 0.02
    static let writeLimit: TimeInterval = 5

    private let channel: CBL2CAPChannel
    private let input: InputStream
    private let output: OutputStream
    private let lock = NSLock()
    private var closed = false
    private var buffer = [UInt8](repeating: 0, count: 16 * 1024)

    init(_ channel: CBL2CAPChannel) {
        self.channel = channel
        input = channel.inputStream
        output = channel.outputStream
        input.open()
        output.open()
    }

    private var broken: Bool {
        lock.lock()
        defer { lock.unlock() }
        if closed { return true }
        return [.atEnd, .closed, .error].contains(input.streamStatus)
    }

    func receive() throws -> Data? {
        let start = Date()
        while true {
            if broken { return nil }
            if input.hasBytesAvailable {
                let count = input.read(&buffer, maxLength: buffer.count)
                if count < 0 { throw PipeError(reason: "블루투스에서 읽지 못했습니다") }
                return count == 0 ? nil : Data(buffer[0 ..< count])
            }
            if Date().timeIntervalSince(start) >= Self.readWait { return Data() }
            Thread.sleep(forTimeInterval: Self.readPoll)
        }
    }

    func send(_ bytes: Data) throws {
        let start = Date()
        var sent = 0
        while sent < bytes.count {
            if broken { throw PipeError(reason: "블루투스 연결이 끊겼습니다") }
            if output.hasSpaceAvailable {
                let wrote = bytes.withUnsafeBytes {
                    output.write($0.bindMemory(to: UInt8.self).baseAddress! + sent, maxLength: bytes.count - sent)
                }
                if wrote < 0 { throw PipeError(reason: "블루투스로 보내지 못했습니다") }
                sent += wrote
                continue
            }
            if Date().timeIntervalSince(start) > Self.writeLimit {
                throw PipeError(reason: "블루투스로 보내는 데 너무 오래 걸립니다")
            }
            Thread.sleep(forTimeInterval: 0.002)
        }
    }

    func close() {
        lock.lock()
        defer { lock.unlock() }
        if closed { return }
        closed = true
        input.close()
        output.close()
    }
}
