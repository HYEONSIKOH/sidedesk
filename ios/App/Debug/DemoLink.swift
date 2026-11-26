#if DEBUG
    import Foundation
    import UIKit

    /// 디버그 빌드 전용. 기기 없이 화면을 찍어 보려고 실행 인자로 보기용 값과 화면을 고른다.
    /// 예: `-demo YES -screen perf -theme Light -face Dial -ask YES -landscape YES`
    /// (`-theme`, `-face` 는 저장 값 대신 이번 실행에만 쓰인다)
    @MainActor
    enum Demo {
        private static var defaults: UserDefaults { .standard }

        static var on: Bool { defaults.bool(forKey: "demo") }

        static func apply(_ world: World) {
            guard on else { return }
            switch defaults.string(forKey: "screen") {
            case "perf": world.desk.show(.perf)
            case "processes": world.desk.show(.processes)
            default: break
            }
            if defaults.string(forKey: "sort") == "memory" { world.desk.sortBy(.memory) }
            if defaults.bool(forKey: "picking") { world.clock.pick() }
            if defaults.bool(forKey: "locationNotice") { world.askingLocation = true }
            if defaults.bool(forKey: "ask"), let first = world.desk.rows.first { world.desk.ask(first) }
            if defaults.bool(forKey: "landscape") {
                let scene = UIApplication.shared.connectedScenes.first as? UIWindowScene
                scene?.requestGeometryUpdate(.iOS(interfaceOrientations: .landscapeRight))
            }
        }
    }

    /// 보기용 연결. 컴퓨터 없이 값을 채운다.
    @MainActor
    final class DemoLink: ComputerLink {
        let feed = LinkFeed()
        private var timer: Timer?
        private var tick = 0

        init() {
            let defaults = UserDefaults.standard
            if let code = defaults.string(forKey: "pairing") {
                feed.state = .pairing(code: code)
            } else if defaults.bool(forKey: "lost") {
                feed.state = .lost(name: "내 맥")
            } else {
                feed.state = .connected(name: "내 맥", via: defaults.bool(forKey: "bluetooth") ? .bluetooth : .usb)
            }
            feed.bluetoothDecided = true
            feed.weather = Weather(
                temperature: 12,
                sky: .clear,
                high: 15,
                low: 6,
                isDay: true,
                fetchedAt: Int64(Date().timeIntervalSince1970 * 1000),
                source: .openMeteo
            )
            feed.processes = [
                ComputerProcess(pid: 101, startedAt: 1, name: "Xcode", cpu: 48.2, memoryBytes: 3_400_000_000),
                ComputerProcess(pid: 102, startedAt: 1, name: "Google Chrome", cpu: 22.5, memoryBytes: 2_100_000_000),
                ComputerProcess(pid: 103, startedAt: 1, name: "빌드", cpu: 11.8, memoryBytes: 820_000_000),
                ComputerProcess(pid: 104, startedAt: 1, name: "Slack", cpu: 6.1, memoryBytes: 640_000_000),
                ComputerProcess(pid: 105, startedAt: 1, name: "터미널", cpu: 3.4, memoryBytes: 120_000_000),
                ComputerProcess(pid: 106, startedAt: 1, name: "Finder", cpu: 1.2, memoryBytes: 210_000_000),
                ComputerProcess(pid: 107, startedAt: 1, name: "음악", cpu: 0.9, memoryBytes: 180_000_000),
                ComputerProcess(pid: 108, startedAt: 1, name: "WindowServer", cpu: 7.3, memoryBytes: 460_000_000),
                ComputerProcess(pid: 109, startedAt: 1, name: "Docker", cpu: 4.4, memoryBytes: 1_500_000_000),
                ComputerProcess(pid: 110, startedAt: 1, name: "메모", cpu: 0.2, memoryBytes: 90_000_000),
            ]
            for _ in 0 ..< 40 { step() }
        }

        private func step() {
            tick += 1
            let wave = sin(Double(tick) / 3)
            feed.receive(
                perf: Perf(
                    cpu: Float(31 + 6 * wave),
                    gpu: 12,
                    memory: 58,
                    disk: 44,
                    receivePerSecond: Int64(18_000 + 9_000 * wave + Double(tick % 7) * 3_000),
                    sendPerSecond: Int64(6_000 + 2_000 * cos(Double(tick) / 2))
                )
            )
        }

        func start() {
            guard timer == nil else { return }
            timer = Timer.scheduledTimer(withTimeInterval: 1, repeats: true) { [weak self] _ in
                MainActor.assumeIsolated { self?.step() }
            }
        }

        func stop() {
            timer?.invalidate()
            timer = nil
        }

        func subscribe(_ subscription: Subscription) {}

        func setLocation(_ location: LocationState) {}

        func kill(_ command: KillCommand) {
            Task { [weak self] in
                try? await Task.sleep(for: .milliseconds(500))
                self?.feed.emit(KillResult(commandId: command.commandId, ok: true))
            }
        }

        func giveUp(_ commandId: UInt64) {}

        func answer(_ same: Bool) {}
    }
#endif
