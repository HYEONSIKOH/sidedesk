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
            case "ai": world.desk.show(.ai)
            case "perf": world.desk.show(.perf)
            case "processes": world.desk.show(.processes)
            case "shortcuts": world.desk.show(.shortcuts)
            case "music": world.desk.show(.music)
            default: break
            }
            if defaults.bool(forKey: "restPanel") {
                world.restOpen = true
                world.clock.pick()
            }
            if defaults.bool(forKey: "rest") { world.rest.forceRest() }
            if defaults.bool(forKey: "alert"), let link = world.link as? DemoLink { link.sendAlert() }
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
            let now = Int64(Date().timeIntervalSince1970 * 1000)
            feed.sessions = [
                AiSession(id: "1", tool: .claudeCode, title: "로그인 화면", project: "app", state: .working, since: now - 12 * 60_000, open: true),
                AiSession(id: "2", tool: .codex, title: "테스트 실행", project: "web", state: .waiting, since: now - 3 * 60_000, open: true),
                AiSession(id: "3", tool: .claudeCode, title: "문서 정리", project: "docs", state: .done, since: now - 64 * 60_000, open: true),
                AiSession(id: "4", tool: .codex, title: "검색 개선", project: "search", state: .done, since: now - 26 * 3_600_000, open: false),
            ]
            feed.forge = Forge(
                github: ForgeCounts(state: .ok, reviews: 2, mine: 3, notices: 57),
                gitlab: ForgeCounts(state: .signedOut, reviews: 0, mine: 0, notices: 0)
            )
            feed.shortcuts = [
                Shortcut(id: 1, name: "터미널", icon: "terminal", color: "slate", toggle: .none, system: false),
                Shortcut(id: 2, name: "다운로드", icon: "folder", color: "green", toggle: .none, system: false),
                Shortcut(id: 3, name: "Xcode", icon: "code", color: "purple", toggle: .none, system: false),
                Shortcut(id: 4, name: "브라우저", icon: "browser", color: "orange", toggle: .none, system: false),
                Shortcut(id: 5, name: "계산기", icon: "calculator", color: "teal", toggle: .none, system: false),
                Shortcut(id: 6, name: "", icon: "activity", color: "olive", toggle: .none, system: false),
                Shortcut(id: 7, name: "소리", icon: "volume", color: "sky", toggle: .off, system: true),
                Shortcut(id: 8, name: "방해 금지", icon: "dnd", color: "sand", toggle: .on, system: true),
                Shortcut(id: 9, name: "잠금", icon: "lock", color: "rose", toggle: .none, system: true),
            ]
            feed.receive(nowPlaying: NowPlaying(
                player: .music, title: "여름 바다", artist: "보기용 아티스트", album: "보기용 앨범",
                durationMs: 221_000, artwork: 0, playing: true, positionMs: 84_000,
                canPlay: true, canNext: true, canPrevious: true, receivedAt: monotonicMillis()
            ))
            feed.volume = Volume(available: true, level: 55, muted: false, settable: true)
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
            // 그래프가 듣기 시작한 뒤 1분치를 채운다.
            if tick == 0 {
                for _ in 0 ..< 60 { step() }
            }
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

        func send(_ command: Command) {
            Task { [weak self] in
                try? await Task.sleep(for: .milliseconds(500))
                self?.feed.emit(CommandResult(commandId: command.commandId, ok: true))
            }
        }

        func runShortcut(_ command: RunShortcut) -> Bool {
            send(.shortcut(command))
            return true
        }

        func music(_ command: MusicCommand) -> Bool {
            send(.music(command))
            return true
        }

        /// 보기용 알림 하나(작은 칸이 커지고 4초 보임)
        func sendAlert() {
            feed.emit(AiAlert(id: 1, sessionId: "2", title: "테스트 실행", kind: .waiting, at: Int64(Date().timeIntervalSince1970 * 1000)))
        }

        func giveUp(_ commandId: UInt64) {}

        func answer(_ same: Bool) {}
    }
#endif
