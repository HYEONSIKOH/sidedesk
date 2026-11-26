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
    case perf
    case processes
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
struct KillResult: Sendable, Equatable {
    var commandId: UInt64
    var ok: Bool
    var reason: String = ""
}

struct KillCommand: Sendable, Equatable {
    var commandId: UInt64
    var pid: Int32
    var startedAt: Int64
}
