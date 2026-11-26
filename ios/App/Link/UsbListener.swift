import Darwin
import Foundation

/// 기기 안 127.0.0.1 의 정해진 포트에서 맥이 usbmuxd 를 거쳐 붙어 오기를 기다린다.
/// 루프백에만 묶어 다른 네트워크에서는 못 붙는다. 같은 폰의 다른 앱은 붙을 수 있으므로
/// 접속은 확인 전(숫자나 Ready 전)에는 아무것도 바꾸지 않는다(`Connection`).
/// 시뮬레이터에서는 열지 않는다. 시뮬레이터의 127.0.0.1 은 맥의 것이라 맥에 포트가 열린다.
final class UsbListener: @unchecked Sendable {
    /// 확인 전 접속을 닫는 시간
    static let readyLimit: TimeInterval = 5

    private let owner: LinkOwner
    private let keys: KeyStore
    private let makeSession: SessionMaker
    private let port: UInt16
    private let lock = NSLock()
    private var server: Int32 = -1
    private var running = false
    private let checking = CheckingSlot()

    init(owner: LinkOwner, keys: KeyStore, makeSession: @escaping SessionMaker, port: UInt16) {
        self.owner = owner
        self.keys = keys
        self.makeSession = makeSession
        self.port = port
    }

    func start() {
        #if targetEnvironment(simulator)
            return
        #else
            lock.lock()
            defer { lock.unlock() }
            if running { return }
            running = true
            let thread = Thread { [self] in open() }
            thread.name = "sidedesk-usb-accept"
            thread.start()
        #endif
    }

    /// 바로 앞에 닫은 대기가 아직 포트를 쥐고 있을 수 있어 몇 번 다시 묶어 본다.
    private func open() {
        for _ in 0 ..< 20 {
            if let fd = Self.listen(port: port) {
                lock.lock()
                guard running, server < 0 else {
                    lock.unlock()
                    Darwin.close(fd)
                    return
                }
                server = fd
                lock.unlock()
                accept(fd)
                return
            }
            lock.lock()
            let wanted = running
            lock.unlock()
            if !wanted { return }
            Thread.sleep(forTimeInterval: 0.25)
        }
    }

    func stop() {
        lock.lock()
        let fd = server
        running = false
        server = -1
        lock.unlock()
        // close 만으로는 기다리는 accept 가 깨지 않는다.
        if fd >= 0 { Darwin.shutdown(fd, SHUT_RDWR) }
        checking.clear()
    }

    private static func listen(port: UInt16) -> Int32? {
        let fd = socket(AF_INET, SOCK_STREAM, 0)
        guard fd >= 0 else { return nil }
        var yes: Int32 = 1
        setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, &yes, socklen_t(MemoryLayout<Int32>.size))
        var address = sockaddr_in()
        address.sin_len = UInt8(MemoryLayout<sockaddr_in>.size)
        address.sin_family = sa_family_t(AF_INET)
        address.sin_port = port.bigEndian
        address.sin_addr.s_addr = in_addr_t(INADDR_LOOPBACK).bigEndian
        let bound = withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                bind(fd, $0, socklen_t(MemoryLayout<sockaddr_in>.size))
            }
        }
        guard bound == 0, Darwin.listen(fd, 4) == 0 else {
            Darwin.close(fd)
            return nil
        }
        return fd
    }

    private func accept(_ fd: Int32) {
        defer { Darwin.close(fd) }
        while true {
            let client = Darwin.accept(fd, nil, nil)
            if client < 0 {
                if errno == EINTR { continue }
                return
            }
            let pipe = SocketPipe(client)
            lock.lock()
            guard running, server == fd else {
                lock.unlock()
                pipe.close()
                return
            }
            lock.unlock()
            checking.replace(with: pipe)
            let thread = Thread { [self] in
                let letGo = { [self] in checking.release(pipe) }
                Connection(
                    pipe: pipe,
                    via: .usb,
                    owner: owner,
                    keys: keys,
                    makeSession: makeSession,
                    readyLimit: Self.readyLimit,
                    afterTake: letGo
                ).run()
                letGo()
            }
            thread.name = "sidedesk-usb"
            thread.start()
        }
    }
}

/// 소켓 하나. 읽기는 250ms 까지 기다린다. 닫기는 끊기만 하고, 파일 번호는 놓을 때 닫는다.
final class SocketPipe: LinkPipe, @unchecked Sendable {
    static let poll: Int32 = 250
    private let fd: Int32
    private let lock = NSLock()
    private var shut = false
    private var buffer = [UInt8](repeating: 0, count: 16 * 1024)

    init(_ fd: Int32) {
        self.fd = fd
        var yes: Int32 = 1
        // 끊긴 소켓에 쓰면 앱이 죽지 않고 오류가 나게 한다.
        setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &yes, socklen_t(MemoryLayout<Int32>.size))
    }

    deinit {
        Darwin.close(fd)
    }

    func receive() throws -> Data? {
        var wait = pollfd(fd: fd, events: Int16(POLLIN), revents: 0)
        let ready = Darwin.poll(&wait, 1, Self.poll)
        if ready < 0 {
            if errno == EINTR { return Data() }
            throw PipeError(reason: "읽지 못함")
        }
        if ready == 0 { return Data() }
        let count = buffer.withUnsafeMutableBytes { read(fd, $0.baseAddress, $0.count) }
        if count < 0 {
            if errno == EINTR || errno == EAGAIN { return Data() }
            throw PipeError(reason: "읽지 못함")
        }
        return count == 0 ? nil : Data(buffer[0 ..< count])
    }

    func send(_ bytes: Data) throws {
        var sent = 0
        while sent < bytes.count {
            let wrote = bytes.withUnsafeBytes { write(fd, $0.baseAddress! + sent, bytes.count - sent) }
            if wrote < 0 {
                if errno == EINTR { continue }
                throw PipeError(reason: "보내지 못함")
            }
            sent += wrote
        }
    }

    func close() {
        lock.lock()
        defer { lock.unlock() }
        if shut { return }
        shut = true
        Darwin.shutdown(fd, SHUT_RDWR)
    }
}

/// 확인 중인 USB 접속 자리 하나. 새 접속이 오면 옛 것을 닫고 새 것을 둔다
/// (폰의 다른 앱이 자리를 쥐고 있어도 맥이 이긴다).
final class CheckingSlot: @unchecked Sendable {
    private let lock = NSLock()
    private var held: LinkPipe?

    func replace(with pipe: LinkPipe) {
        lock.lock()
        let old = held
        held = pipe
        lock.unlock()
        old?.close()
    }

    /// 이 접속이 자리를 쥐고 있을 때만 놓는다.
    func release(_ pipe: LinkPipe) {
        lock.lock()
        if held === pipe { held = nil }
        lock.unlock()
    }

    /// 앱이 뒤로 갈 때. 확인 중인 접속을 닫는다.
    func clear() {
        lock.lock()
        let old = held
        held = nil
        lock.unlock()
        old?.close()
    }

    var isHeld: Bool {
        lock.lock()
        defer { lock.unlock() }
        return held != nil
    }
}
