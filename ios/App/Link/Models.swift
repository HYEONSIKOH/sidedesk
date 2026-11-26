import Foundation

/// 컴퓨터와 이어진 길
enum Via: Sendable, Equatable {
    case usb
    case bluetooth
}

enum LinkState: Equatable, Sendable {
    case waiting
    /// 컴퓨터에 뜬 번호와 같은지 사람이 확인해야 한다.
    case pairing(code: String)
    case connected(name: String?, via: Via)
    /// 끊겼다. 마지막 이름을 흐리게 보인다.
    case lost(name: String?)
    case failed(reason: String)
}

/// 컴퓨터가 모아 보낼 수 있는 것. 시계는 폰 시각을 써서 없다.
enum Topic: Sendable, Hashable {
    case weather
    case ai
    case perf
    case processes
    case shortcuts
    case music
}

enum Sky: Sendable, Equatable {
    case unknown, clear, partlyCloudy, cloudy, fog, drizzle, rain, snow, shower, thunderstorm
}

/// 날씨를 실제로 받은 곳. unknown 이면 출처를 숨긴다.
enum WeatherSource: Sendable, Equatable {
    case unknown, kma, openMeteo
}

/// 기온은 °C, fetchedAt 은 컴퓨터가 받은 시각(유닉스 밀리초)
struct Weather: Sendable, Equatable {
    var temperature: Float
    var sky: Sky
    var high: Float
    var low: Float
    var isDay: Bool
    var fetchedAt: Int64
    var source: WeatherSource
}

/// 기기 위치. 반올림과 바뀜 판단은 core 가 한다.
enum LocationState: Sendable, Equatable {
    /// 아직 모른다. 보내지 않는다.
    case unknown
    /// 권한이 없다. 위치 없음을 보낸다.
    case off
    case at(latitude: Double, longitude: Double)
}

/// 지금 보이는 것. 큰 화면은 하나, 작은 칸은 여럿. 둘 다 비면 컴퓨터는 수집을 멈춘다.
struct Subscription: Sendable, Equatable {
    var big: Topic?
    var small: Set<Topic>

    init(big: Topic? = nil, small: Set<Topic> = []) {
        self.big = big
        self.small = small
    }

    static let none = Subscription()
}

/// 사용률은 0~100. GPU 를 못 읽는 컴퓨터는 nil
struct Perf: Sendable, Equatable {
    var cpu: Float
    var gpu: Float?
    var memory: Float
    var disk: Float
    var receivePerSecond: Int64
    var sendPerSecond: Int64
}

/// cpu 는 코어 하나가 100 이다. (Foundation 의 ProcessInfo 와 겹치지 않게 이름을 달리 둔다)
struct ComputerProcess: Sendable, Equatable, Identifiable {
    var pid: Int32
    var startedAt: Int64
    var name: String
    var cpu: Float
    var memoryBytes: Int64

    var id: String { "\(pid)-\(startedAt)" }
}

/// 실패면 컴퓨터가 보낸 이유 한 문장
struct CommandResult: Sendable, Equatable {
    var commandId: UInt64
    var ok: Bool
    var reason: String = ""
}

struct KillCommand: Sendable, Equatable {
    var commandId: UInt64
    var pid: Int32
    var startedAt: Int64
}

/// book 은 목록을 받을 때 온 그 컴퓨터의 장부 번호다. LinkOwner 가 채운다.
struct RunShortcut: Sendable, Equatable {
    var commandId: UInt64
    var shortcutId: UInt32
    var book: UInt64 = 0
}

enum MusicAction: Sendable, Equatable {
    case playPause, next, previous, setVolume
}

/// level 은 setVolume 일 때만 쓴다.
struct MusicCommand: Sendable, Equatable {
    var commandId: UInt64
    var action: MusicAction
    var level: Int = 0
}

/// 컴퓨터에 보내는 명령. 번호마다 한 번만 실행되고 결과가 하나 온다.
enum Command: Sendable, Equatable {
    case kill(KillCommand)
    case shortcut(RunShortcut)
    case music(MusicCommand)

    var commandId: UInt64 {
        switch self {
        case .kill(let command): command.commandId
        case .shortcut(let command): command.commandId
        case .music(let command): command.commandId
        }
    }
}

enum AiTool: Sendable, Equatable {
    case unknown, claudeCode, codex
}

/// 세션 단계. unknown 은 기록 모양을 몰라 정하지 못한 것이다.
enum AiState: Sendable, Equatable {
    case unknown, working, waiting, done
}

/// since 는 지금 단계가 된 때(유닉스 밀리초), open 은 프로그램이 아직 떠 있는지
struct AiSession: Sendable, Equatable, Identifiable {
    var id: String
    var tool: AiTool
    var title: String
    var project: String
    var state: AiState
    var since: Int64
    var open: Bool

    /// 작업 중과 답변 대기는 큰 카드
    var active: Bool { state == .working || state == .waiting }
}

enum AlertKind: Sendable, Equatable {
    case unknown, done, waiting
}

/// 작업 중에서 끝남이나 답변 대기로 바뀐 순간. id 는 컴퓨터가 늘려 간다.
struct AiAlert: Sendable, Equatable {
    var id: UInt64
    var sessionId: String
    var title: String
    var kind: AlertKind
    var at: Int64
}

enum ForgeState: Sendable, Equatable {
    case unknown, ok, missing, signedOut, failed
}

/// 검토 요청, 내가 연 요청, 읽지 않은 알림 수
struct ForgeCounts: Sendable, Equatable {
    var state: ForgeState
    var reviews: Int
    var mine: Int
    var notices: Int

    static let unknown = ForgeCounts(state: .unknown, reviews: 0, mine: 0, notices: 0)
}

struct Forge: Sendable, Equatable {
    var github: ForgeCounts
    var gitlab: ForgeCounts
}

enum MusicPlayer: Sendable, Equatable {
    case none, music, spotify, system
}

/// 지금 재생 중인 곡. player 가 none 이면 곡이 없다.
/// receivedAt 은 이 값을 받은 폰의 단조 시각(밀리초)이다. 진행 바는 이것을 기준으로 폰이 스스로 움직인다.
struct NowPlaying: Sendable, Equatable {
    var player: MusicPlayer
    var title: String
    var artist: String
    var album: String
    /// 0 은 모름(라디오, 생방송)
    var durationMs: Int64
    /// 그림 해시. 0 은 없음
    var artwork: UInt64
    var playing: Bool
    var positionMs: Int64
    var canPlay: Bool
    var canNext: Bool
    var canPrevious: Bool
    var receivedAt: Int64 = 0
}

struct Volume: Sendable, Equatable {
    var available: Bool
    var level: Int
    var muted: Bool
    var settable: Bool
}

/// 소리 끄기, 방해 금지처럼 켜짐 상태가 있는 버튼. none 은 누르면 한 번 하는 것
enum Toggle: Sendable, Equatable {
    case none, on, off
}

/// 등록 내용(앱, 경로, 주소, 키)은 오지 않는다. 폰은 이름, 그림, 색만 안다.
struct Shortcut: Sendable, Equatable, Identifiable {
    var id: UInt32
    var name: String
    var icon: String
    var color: String
    var toggle: Toggle
    /// 시스템 동작(소리, 방해 금지, 잠금)이면 칸에 색 그림만 그린다.
    var system: Bool
}
