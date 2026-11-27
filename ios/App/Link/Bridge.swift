import Foundation
import SideDeskBridge

/// core 의 값과 함수. 바인딩 모듈(`SideDeskBridge`)은 이 파일만 가져온다.
enum Core {
    static var usbPort: UInt16 { SideDeskBridge.usbPort() }
    static var serviceUuid: String { bleServiceUuid() }
    static var infoUuid: String { bleInfoUuid() }

    static func serviceData(psm: UInt16, publicKey: Data) throws -> Data {
        try bleServiceData(psm: psm, publicKey: publicKey)
    }

    static func newKeyPair() -> Identity {
        let pair = generateKeyPair()
        return Identity(privateKey: pair.privateKey, publicKey: pair.publicKey)
    }

    static func newRequestId() -> UInt64 {
        SideDeskBridge.newRequestId()
    }

    static let makeSession: SessionMaker = { identity, computers, allowPairing in
        do {
            let session = try PhoneSession(
                privateKey: identity.privateKey,
                publicKey: identity.publicKey,
                computers: computers,
                allowPairing: allowPairing
            )
            session.setName(name: DeviceModel.name)
            return BridgeSession(session)
        } catch {
            throw CoreErrors.map(error)
        }
    }
}

enum CoreErrors {
    static func map(_ error: Error) -> Error {
        guard let error = error as? LinkError else { return error }
        switch error {
        case .Version: return CoreError.version
        case .NotPaired: return CoreError.notPaired
        case .Rejected: return CoreError.rejected
        case .Broken(let reason): return CoreError.broken(reason)
        }
    }

    static func wrap<T>(_ work: () throws -> T) throws -> T {
        do {
            return try work()
        } catch {
            throw map(error)
        }
    }
}

/// core 세션을 앱 형으로 감싼다.
final class BridgeSession: CoreSession {
    private let session: PhoneSession

    init(_ session: PhoneSession) {
        self.session = session
    }

    func feed(_ bytes: Data) throws {
        try CoreErrors.wrap { try session.feed(bytes: bytes) }
    }

    func takeOutput() -> Data {
        session.takeOutput()
    }

    func nextEvent() -> CoreEvent? {
        while let event = session.nextEvent() {
            if let mapped = Self.map(event) { return mapped }
        }
        return nil
    }

    func confirm(_ same: Bool) throws {
        try CoreErrors.wrap { try session.confirm(same: same) }
    }

    func subscribe(main: Topic?, tiles: [Topic]) throws {
        try CoreErrors.wrap { try session.subscribe(main: main.map(Self.view), tiles: tiles.map(Self.view)) }
    }

    func sendLocation(latitude: Double, longitude: Double) throws {
        try CoreErrors.wrap { try session.sendLocation(place: Place(latitude: latitude, longitude: longitude)) }
    }

    func sendNoLocation() throws {
        try CoreErrors.wrap { try session.sendLocation(place: nil) }
    }

    func runShortcut(requestId: UInt64, shortcutId: UInt32, book: UInt64) throws {
        try CoreErrors.wrap { try session.runShortcut(requestId: requestId, shortcutId: shortcutId, book: book) }
    }

    func musicCommand(requestId: UInt64, action: MusicAction, level: Int) throws {
        let core: SideDeskBridge.MusicAction = switch action {
        case .playPause: .playPause
        case .next: .next
        case .previous: .previous
        case .setVolume: .setVolume
        }
        try CoreErrors.wrap {
            try session.musicCommand(requestId: requestId, action: core, level: UInt32(clamping: max(0, min(100, level))))
        }
    }

    func killProcess(requestId: UInt64, pid: Int32, startedAt: Int64) throws {
        try CoreErrors.wrap {
            try session.killProcess(requestId: requestId, pid: UInt32(bitPattern: pid), startedAt: UInt64(max(0, startedAt)))
        }
    }

    private static func view(_ topic: Topic) -> SideDeskBridge.View {
        switch topic {
        case .weather: .weather
        case .ai: .ai
        case .perf: .performance
        case .processes: .processes
        case .shortcuts: .shortcuts
        case .music: .music
        }
    }

    /// 모르는 값(다른 티켓이 core 에 더한 일)은 버린다.
    static func map(_ event: PhoneEvent) -> CoreEvent? {
        switch event {
        case .pairingCode(let code):
            return .pairingCode(code)
        case .paired(let key):
            return .paired(computerKey: key)
        case .ready(let key):
            return .ready(computerKey: key)
        case .computerName(let name):
            return .computerName(name)
        case .performance(let value):
            return .performance(
                Perf(
                    cpu: value.cpu,
                    gpu: value.gpu,
                    memory: value.memory,
                    disk: value.disk,
                    receivePerSecond: Int64(clamping: value.netDown),
                    sendPerSecond: Int64(clamping: value.netUp)
                )
            )
        case .processes(let items):
            return .processes(
                items.map {
                    ComputerProcess(
                        pid: Int32(bitPattern: $0.pid),
                        startedAt: Int64(clamping: $0.startedAt),
                        name: $0.name,
                        cpu: $0.cpu,
                        memoryBytes: Int64(clamping: $0.memory)
                    )
                }
            )
        case .commandResult(let id, let ok, let reason):
            return .commandResult(requestId: id, ok: ok, reason: reason)
        case .weather(let value):
            return .weather(
                Weather(
                    temperature: value.temperature,
                    sky: sky(value.sky),
                    high: value.high,
                    low: value.low,
                    isDay: value.isDay,
                    fetchedAt: Int64(clamping: value.fetchedAt),
                    source: source(value.source)
                )
            )
        case .aiSessions(let items):
            return .aiSessions(items.map(session))
        case .aiAlert(let value):
            let kind: AlertKind = switch value.kind {
            case .done: .done
            case .waiting: .waiting
            default: .unknown
            }
            return .aiAlert(AiAlert(id: value.id, sessionId: value.sessionId, title: value.title, kind: kind, at: Int64(clamping: value.at)))
        case .forge(let github, let gitlab):
            return .forge(Forge(github: forge(github), gitlab: forge(gitlab)))
        case .shortcuts(let book, let items):
            return .shortcuts(book: book, items: items.map(shortcut))
        case .nowPlaying(let value):
            return .nowPlaying(nowPlaying(value))
        case .artwork(let hash, let jpeg):
            return .artwork(hash: hash, jpeg: jpeg)
        case .volume(let value):
            return .volume(Volume(available: value.available, level: Int(value.level), muted: value.muted, settable: value.settable))
        default:
            // 다음 티켓이 core 에 더하는 일처럼 iOS 화면이 아직 없는 일은 버린다.
            return nil
        }
    }

    private static func session(_ it: AiSessionInfo) -> AiSession {
        let tool: AiTool = switch it.tool {
        case .claudeCode: .claudeCode
        case .codex: .codex
        default: .unknown
        }
        let state: AiState = switch it.state {
        case .working: .working
        case .waiting: .waiting
        case .done: .done
        default: .unknown
        }
        return AiSession(id: it.id, tool: tool, title: it.title, project: it.project, state: state, since: Int64(clamping: it.since), open: it.open)
    }

    /// 받은 적 없는 쪽은 모름으로 둔다.
    private static func forge(_ it: ForgeInfo?) -> ForgeCounts {
        guard let it else { return .unknown }
        let state: ForgeState = switch it.state {
        case .ok: .ok
        case .missing: .missing
        case .signedOut: .signedOut
        case .failed: .failed
        default: .unknown
        }
        return ForgeCounts(state: state, reviews: Int(it.reviews), mine: Int(it.mine), notices: Int(it.notices))
    }

    private static func shortcut(_ it: ShortcutInfo) -> Shortcut {
        let toggle: Toggle = switch it.toggle {
        case .on: .on
        case .off: .off
        default: .none
        }
        return Shortcut(id: it.id, name: it.name, icon: it.icon, color: it.color, toggle: toggle, system: it.system)
    }

    private static func nowPlaying(_ it: NowPlayingInfo) -> NowPlaying {
        let player: SideDesk.MusicPlayer = switch it.player {
        case .music: .music
        case .spotify: .spotify
        case .system: .system
        default: .none
        }
        return NowPlaying(
            player: player,
            title: it.title,
            artist: it.artist,
            album: it.album,
            durationMs: Int64(clamping: it.durationMs),
            artwork: it.artwork,
            playing: it.playing,
            positionMs: Int64(clamping: it.positionMs),
            canPlay: it.canPlay,
            canNext: it.canNext,
            canPrevious: it.canPrevious
        )
    }

    private static func sky(_ sky: SideDeskBridge.Sky) -> Sky {
        switch sky {
        case .unknown: .unknown
        case .clear: .clear
        case .partlyCloudy: .partlyCloudy
        case .cloudy: .cloudy
        case .fog: .fog
        case .drizzle: .drizzle
        case .rain: .rain
        case .snow: .snow
        case .shower: .shower
        case .thunderstorm: .thunderstorm
        @unknown default: .unknown
        }
    }

    private static func source(_ source: SideDeskBridge.WeatherSource) -> WeatherSource {
        switch source {
        case .kma: .kma
        case .openMeteo: .openMeteo
        case .unknown: .unknown
        @unknown default: .unknown
        }
    }
}
