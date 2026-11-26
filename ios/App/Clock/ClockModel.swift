import Foundation
import Observation

enum Face: String, CaseIterable, Sendable {
    case digits = "Digits"
    case dial = "Dial"
    case weather = "Weather"
    case calendar = "Calendar"
    case overview = "Overview"

    var label: String {
        switch self {
        case .digits: "숫자"
        case .dial: "바늘"
        case .weather: "날씨"
        case .calendar: "달력"
        case .overview: "모아 보기"
        }
    }
}

enum Theme: String, CaseIterable, Sendable {
    case light = "Light"
    case dark = "Dark"
}

/// 기기에 남기는 고른 값. 앱을 껐다 켜도 남는다.
@MainActor
protocol Settings: AnyObject {
    var face: String? { get set }
    var theme: String? { get set }
    /// 위치 안내를 한 번 보였는지
    var askedLocation: Bool { get set }
}

@MainActor
final class DefaultsSettings: Settings {
    private let defaults: UserDefaults

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
    }

    var face: String? {
        get { defaults.string(forKey: "face") }
        set { defaults.set(newValue, forKey: "face") }
    }

    var theme: String? {
        get { defaults.string(forKey: "theme") }
        set { defaults.set(newValue, forKey: "theme") }
    }

    var askedLocation: Bool {
        get { defaults.bool(forKey: "askedLocation") }
        set { defaults.set(newValue, forKey: "askedLocation") }
    }
}

/// 홈 시계의 모양, 테마, 고르기, 날짜 옆 아이콘, 날씨(안드로이드 `ClockViewModel`)
@MainActor
@Observable
final class ClockModel {
    /// 폰과 맥의 시계 차이를 생각해 1시간 넘게 지난 값만 오래된 것으로 본다.
    nonisolated static let staleMillis: Int64 = 60 * 60 * 1000

    private(set) var face: Face
    private(set) var theme: Theme
    private(set) var picking = false
    /// 보이는 동안 1분마다 올려 오래된 날씨 판단을 다시 하게 한다.
    private(set) var minute = 0

    @ObservationIgnored private let link: any ComputerLink
    @ObservationIgnored private let settings: any Settings
    @ObservationIgnored private let now: () -> Date
    @ObservationIgnored private var ticking: Task<Void, Never>?

    init(link: any ComputerLink, settings: any Settings, now: @escaping () -> Date = Date.init) {
        self.link = link
        self.settings = settings
        self.now = now
        face = Face(rawValue: settings.face ?? "") ?? .digits
        theme = Theme(rawValue: settings.theme ?? "") ?? .dark
    }

    /// 날짜 옆 아이콘. 끊기면 nil
    var via: Via? {
        if case .connected(_, let via) = link.feed.state { via } else { nil }
    }

    var weather: Weather? { link.feed.weather }

    /// 끊겼거나 받은 지 오래된 날씨
    var weatherDimmed: Bool {
        _ = minute
        guard case .connected = link.feed.state else { return true }
        guard let weather else { return false }
        return Int64(now().timeIntervalSince1970 * 1000) - weather.fetchedAt > Self.staleMillis
    }

    /// 지금 기온. 한 번도 못 받았으면(위치 권한이 없으면) --°
    var temperature: String {
        weather.map { degrees($0.temperature) } ?? "--°"
    }

    /// 날씨 옆 출처. 날씨가 없거나 받은 곳을 모르면 nil
    var credit: String? {
        switch weather?.source {
        case .kma: "날씨 자료 기상청"
        case .openMeteo: "날씨 자료 Open-Meteo.com"
        default: nil
        }
    }

    /// 작은 칸에 쓰는 짧은 출처
    var shortCredit: String? {
        switch weather?.source {
        case .kma: "기상청"
        case .openMeteo: "Open-Meteo"
        default: nil
        }
    }

    /// 화면이 보이는지. 안 보이면 1분 확인을 멈춘다.
    func setVisible(_ visible: Bool) {
        ticking?.cancel()
        ticking = nil
        guard visible else { return }
        minute += 1
        ticking = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(60))
                if Task.isCancelled { return }
                self?.minute += 1
            }
        }
    }

    var isTicking: Bool { ticking != nil }

    func pick() {
        picking = true
    }

    func done() {
        picking = false
    }

    func setFace(_ face: Face) {
        settings.face = face.rawValue
        self.face = face
    }

    func setTheme(_ theme: Theme) {
        settings.theme = theme.rawValue
        self.theme = theme
    }

    /// 고르는 중 옆으로 밀면 모양이 돈다.
    func nextFace() {
        turn(1)
    }

    func previousFace() {
        turn(Face.allCases.count - 1)
    }

    private func turn(_ step: Int) {
        guard picking else { return }
        let all = Face.allCases
        let index = all.firstIndex(of: face) ?? 0
        setFace(all[(index + step) % all.count])
    }
}
