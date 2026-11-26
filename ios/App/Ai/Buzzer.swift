import CoreHaptics
import Foundation

/// 알림과 버튼 진동. 시험에서는 가짜를 쓴다.
@MainActor
protocol Buzzer: AnyObject {
    func buzz(_ kind: AlertKind)
    /// 버튼을 눌렀을 때 짧게 한 번
    func tap()
}

/// 진동 세기. 안드로이드 진폭 80, 160, 255 를 255 로 나눈 값
enum Vibration: String, CaseIterable, Sendable {
    case off = "Off"
    case weak = "Weak"
    case normal = "Normal"
    case strong = "Strong"

    var label: String {
        switch self {
        case .off: "끔"
        case .weak: "약하게"
        case .normal: "보통"
        case .strong: "강하게"
        }
    }

    var intensity: Float {
        switch self {
        case .off: 0
        case .weak: 80 / 255
        case .normal: 160 / 255
        case .strong: 1
        }
    }
}

/// 진동 모양(초 단위 시작과 길이). 끝남은 짧게 한 번, 답변 대기는 두 번, 버튼은 아주 짧게. 끔이면 nil
func hapticPulses(_ kind: AlertKind?, _ vibration: Vibration) -> [(start: Double, length: Double)]? {
    if vibration == .off { return nil }
    switch kind {
    case nil: return [(0, 0.02)]
    case .waiting?: return [(0, 0.06), (0.18, 0.06)]
    default: return [(0, 0.08)]
    }
}

/// Core Haptics 진동. 햅틱이 없는 기기(시뮬레이터 포함)에서는 아무것도 하지 않는다.
@MainActor
final class PhoneBuzzer: Buzzer {
    private let vibration: () -> Vibration
    private var engine: CHHapticEngine?

    init(vibration: @escaping () -> Vibration) {
        self.vibration = vibration
    }

    static var supported: Bool { CHHapticEngine.capabilitiesForHardware().supportsHaptics }

    /// 앱이 앞에 올 때 엔진을 켠다.
    func start() {
        guard Self.supported else { return }
        if engine == nil {
            guard let made = try? CHHapticEngine() else { return }
            made.playsHapticsOnly = true
            // 시스템이 엔진을 멈추거나 되살리면 다시 켠다.
            made.resetHandler = { [weak made] in try? made?.start() }
            made.stoppedHandler = { [weak made] _ in try? made?.start() }
            engine = made
        }
        try? engine?.start()
    }

    func stop() {
        engine?.stop()
    }

    func buzz(_ kind: AlertKind) {
        play(kind)
    }

    func tap() {
        play(nil)
    }

    private func play(_ kind: AlertKind?) {
        let strength = vibration()
        guard let engine, let pulses = hapticPulses(kind, strength) else { return }
        let events = pulses.map { pulse in
            CHHapticEvent(
                eventType: .hapticContinuous,
                parameters: [
                    CHHapticEventParameter(parameterID: .hapticIntensity, value: strength.intensity),
                    CHHapticEventParameter(parameterID: .hapticSharpness, value: 0.5),
                ],
                relativeTime: pulse.start,
                duration: pulse.length
            )
        }
        guard let pattern = try? CHHapticPattern(events: events, parameters: []),
              let player = try? engine.makePlayer(with: pattern)
        else { return }
        try? player.start(atTime: CHHapticTimeImmediate)
    }
}
