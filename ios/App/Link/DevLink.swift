#if DEBUG && targetEnvironment(simulator)
import Darwin
import Foundation
import Observation

/// 암호화가 없는 개발용 연결. SD-2 에서 지운다.
@MainActor
@Observable
final class DevLink: ComputerLink {
    private(set) var state = LinkState.waiting

    // 줄이 하나라서 이전 소켓을 닫은 뒤에만 다시 연다.
    @ObservationIgnored private let queue = DispatchQueue(label: "sidedesk.link")
    @ObservationIgnored private var session: Session?

    func start() {
        guard session == nil else { return }
        guard let session = Session() else {
            state = .failed("연결을 열지 못함")
            return
        }
        self.session = session
        queue.async {
            session.run { state in
                Task { @MainActor in self.apply(state) }
            }
        }
    }

    func stop() {
        session?.close()
        session = nil
    }

    private func apply(_ new: LinkState) {
        if new == .waiting, case .received = state { return }
        state = new
    }
}

private final class Session: Sendable {
    private static let path = "/tmp/sidedesk-dev.sock"
    private static let readTimeout: Int32 = 5_000

    private let wakeReader: FileHandle
    private let wakeWriter: FileHandle

    init?() {
        var ends: [Int32] = [-1, -1]
        guard pipe(&ends) == 0 else { return nil }
        wakeReader = FileHandle(fileDescriptor: ends[0], closeOnDealloc: true)
        wakeWriter = FileHandle(fileDescriptor: ends[1], closeOnDealloc: true)
    }

    func close() {
        try? wakeWriter.close()
    }

    func run(report: (LinkState) -> Void) {
        guard let server = open() else {
            report(.failed("연결을 열지 못함"))
            return
        }
        report(.waiting)
        while ready(server, timeout: -1) {
            let client = accept(server, nil, nil)
            guard client >= 0 else { continue }
            if isSameUser(client) { read(client, report: report) }
            Darwin.close(client)
        }
        Darwin.close(server)
        unlink(Self.path)
    }

    private func open() -> Int32? {
        var address = sockaddr_un()
        address.sun_family = sa_family_t(AF_UNIX)
        withUnsafeMutableBytes(of: &address.sun_path) { $0.copyBytes(from: Self.path.utf8) }
        let server = socket(AF_UNIX, SOCK_STREAM, 0)
        guard server >= 0 else { return nil }
        unlink(Self.path)
        let bound = withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                bind(server, $0, socklen_t(MemoryLayout<sockaddr_un>.size))
            }
        }
        guard bound == 0, chmod(Self.path, 0o600) == 0, listen(server, 4) == 0 else {
            Darwin.close(server)
            return nil
        }
        return server
    }

    /// 읽을 것이 있으면 true, 시간이 지났거나 닫으라고 하면 false.
    private func ready(_ descriptor: Int32, timeout: Int32) -> Bool {
        var polled = [
            pollfd(fd: descriptor, events: Int16(POLLIN), revents: 0),
            pollfd(fd: wakeReader.fileDescriptor, events: Int16(POLLIN), revents: 0),
        ]
        while true {
            let count = poll(&polled, 2, timeout)
            if count < 0, errno == EINTR { continue }
            return count > 0 && polled[1].revents == 0
        }
    }

    private func isSameUser(_ client: Int32) -> Bool {
        var user = uid_t()
        var group = gid_t()
        return getpeereid(client, &user, &group) == 0 && user == getuid()
    }

    private func read(_ client: Int32, report: (LinkState) -> Void) {
        let receiver = PhoneReceiver()
        var buffer = [UInt8](repeating: 0, count: 4096)
        while ready(client, timeout: Self.readTimeout) {
            let count = Darwin.read(client, &buffer, buffer.count)
            guard count > 0, let names = try? receiver.receive(bytes: Data(buffer[..<count])) else {
                return
            }
            if let name = names.last { report(.received(name)) }
        }
    }
}
#endif
