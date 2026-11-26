import Foundation
import Observation

/// 앱이 앞에 있을 때 화면을 켜 둘지
enum KeepOn: String, CaseIterable, Sendable {
    case always = "Always"
    case charging = "Charging"
    case never = "Never"

    var label: String {
        switch self {
        case .always: "늘"
        case .charging: "충전 중"
        case .never: "안 함"
        }
    }
}

/// 손을 안 댄 뒤 쉴 때까지. nil 은 쉬지 않음
enum RestAfter: String, CaseIterable, Sendable {
    case one = "One"
    case five = "Five"
    case fifteen = "Fifteen"
    case never = "Never"

    var label: String {
        switch self {
        case .one: "1분"
        case .five: "5분"
        case .fifteen: "15분"
        case .never: "안 함"
        }
    }

    var duration: Duration? {
        switch self {
        case .one: .seconds(60)
        case .five: .seconds(300)
        case .fifteen: .seconds(900)
        case .never: nil
        }
    }
}

/// "늘 켜 두기"여도 충전 중이 아니고 이보다 낮으면 풂
let lowBattery = 15

/// percent 가 nil(모름, 시뮬레이터)이면 100 으로 본다(안드로이드와 같음).
func keepScreenOn(_ setting: KeepOn, charging: Bool, percent: Int?) -> Bool {
    switch setting {
    case .always: charging || (percent ?? 100) >= lowBattery
    case .charging: charging
    case .never: false
    }
}

/// `UIDevice.batteryLevel`(0~1, 모르면 -1)에서 퍼센트
func batteryPercent(_ level: Float) -> Int? {
    level < 0 ? nil : Int((level * 100).rounded())
}

/// 번인을 막는 시계 옮김(pt). 1분마다 가운데 둘레 ±12pt 안의 9자리를 차례로 돈다.
func shiftAt(minute: Int64) -> (x: Double, y: Double) {
    let spots: [(Double, Double)] = [(0, 0), (12, 0), (12, 12), (0, 12), (-12, 12), (-12, 0), (-12, -12), (0, -12), (12, -12)]
    let index = Int(((minute % 9) + 9) % 9)
    return spots[index]
}

/// 쉬지 않게 막는 까닭. 하나라도 켜져 있으면 쉬지 않는다.
enum Hold: Sendable, Hashable {
    case picking, confirm, pairing, noFocus
}

/// 손을 안 대면 쉬고, 손대면 깬다(안드로이드 `RestViewModel`). 진동은 하지 않는다(AI 쪽 한 곳에서만 울린다).
@MainActor
@Observable
final class RestModel {
    private(set) var resting = false
    private(set) var keepOn: KeepOn
    private(set) var restAfter: RestAfter
    private(set) var wakeOnAlert: Bool
    private(set) var vibration: Vibration
    /// 쉬는 화면에 어둡게 보이는 알림
    private(set) var alert: AiAlert?

    @ObservationIgnored private let settings: any Settings
    @ObservationIgnored private var holds = Set<Hold>()
    @ObservationIgnored private var visible = false
    @ObservationIgnored private var timer: Task<Void, Never>?
    @ObservationIgnored private var restingListeners: [(Bool) -> Void] = []

    init(settings: any Settings) {
        self.settings = settings
        keepOn = KeepOn(rawValue: settings.keepOn ?? "") ?? .charging
        restAfter = RestAfter(rawValue: settings.restAfter ?? "") ?? .five
        wakeOnAlert = settings.wakeOnAlert
        vibration = Vibration(rawValue: settings.vibration ?? "") ?? .normal
    }

    /// 쉼이 바뀔 때마다 부른다(구독을 바꾸고 켜 두기를 다시 건다).
    func onResting(_ listener: @escaping (Bool) -> Void) {
        restingListeners.append(listener)
    }

    private func set(resting value: Bool) {
        guard resting != value else { return }
        resting = value
        for listener in restingListeners {
            listener(value)
        }
    }

    /// AI 알림. 쉬는 중이 아니면 아무것도 안 한다. 알림에 깨우기면 깨우고(늦은 알림 빼고), 아니면 아래에 한 줄.
    func alerted(_ value: AiAlert, late: Bool) {
        guard resting else { return }
        if wakeOnAlert, !late {
            touched()
        } else {
            alert = value
        }
    }

    /// 알림이 사라지면 쉬는 화면의 줄도 지운다.
    func alertCleared() {
        alert = nil
    }

    /// 누름마다 부른다. 쉬는 중이면 깨우고, 타이머를 다시 잰다.
    func touched() {
        alert = nil
        set(resting: false)
        restart()
    }

    /// 짝 맺기 번호나 확인 창이 생기면 쉬는 중이어도 깨워 가리지 않는다. 초점 없음은 깨우지 않는다.
    func hold(_ reason: Hold, _ on: Bool) {
        let changed = on ? holds.insert(reason).inserted : holds.remove(reason) != nil
        guard changed else { return }
        if on, reason == .pairing || reason == .confirm {
            alert = nil
            set(resting: false)
        }
        restart()
    }

    /// 화면이 안 보이면 타이머를 멈추고 깨어 있는 상태로 둔다.
    func setVisible(_ value: Bool) {
        visible = value
        if !value {
            alert = nil
            set(resting: false)
        }
        restart()
    }

    func setKeepOn(_ value: KeepOn) {
        settings.keepOn = value.rawValue
        keepOn = value
    }

    func setRestAfter(_ value: RestAfter) {
        settings.restAfter = value.rawValue
        restAfter = value
        restart()
    }

    func setWakeOnAlert(_ value: Bool) {
        settings.wakeOnAlert = value
        wakeOnAlert = value
    }

    func setVibration(_ value: Vibration) {
        settings.vibration = value.rawValue
        vibration = value
    }

    var isWaiting: Bool { timer != nil }

    #if DEBUG
        /// 화면 찍기용. 기다리지 않고 바로 쉰다.
        func forceRest() {
            timer?.cancel()
            timer = nil
            set(resting: true)
        }
    #endif

    private func restart() {
        timer?.cancel()
        timer = nil
        guard let wait = restAfter.duration, visible, holds.isEmpty, !resting else { return }
        timer = Task { [weak self] in
            try? await Task.sleep(for: wait)
            guard !Task.isCancelled, let self else { return }
            timer = nil
            set(resting: true)
        }
    }
}
