import Observation
import SwiftUI
import UIKit

@main
struct SideDeskApp: App {
    @Environment(\.scenePhase) private var phase
    @State private var world = World()

    var body: some Scene {
        WindowGroup {
            RootView(world: world)
        }
        .onChange(of: phase, initial: true) { _, phase in
            // 시스템 권한 창이 앞에 있으면(inactive) 그 안의 누름을 앱이 못 보므로 쉬지 않는다.
            world.rest.hold(.noFocus, phase == .inactive)
            switch phase {
            case .active: world.foreground()
            case .background: world.background()
            default: break
            }
        }
    }
}

/// 앱 전체의 연결과 모델. 화면이 꺼지거나 앱이 뒤로 가면 컴퓨터가 수집을 멈추게 구독을 비운다.
@MainActor
@Observable
final class World {
    static let locationNotice = Confirm(
        title: "날씨에 대략적인 위치를 씁니다",
        description: "대략적인 위치(소수 첫째 자리)를 컴퓨터를 거쳐 날씨 제공처(기상청 또는 Open-Meteo)로 보냅니다",
        confirmLabel: "다음",
        cancelLabel: "사용 안 함",
        glyph: "⌖"
    )

    let link: any ComputerLink
    let home: HomeModel
    let desk: DeskModel
    let clock: ClockModel
    let ai: AiModel
    let shortcuts: ShortcutModel
    let music: MusicModel
    let rest: RestModel
    /// 위치 권한을 묻기 전에 한 번 보이는 안내
    var askingLocation = false
    /// 고르기 판을 쉬기 설정부터 열지(그림 시험용)
    var restOpen = false

    @ObservationIgnored private let settings: any Settings
    @ObservationIgnored private let location: any LocationPermission
    @ObservationIgnored private var offered = false
    @ObservationIgnored private var started = false

    convenience init() {
        #if DEBUG
            let link: any ComputerLink = Demo.on ? DemoLink() : PhoneLink()
        #else
            let link: any ComputerLink = PhoneLink()
        #endif
        self.init(link: link, settings: DefaultsSettings(), location: CoarseLocation(link: link))
    }

    init(link: any ComputerLink, settings: any Settings, location: any LocationPermission) {
        self.link = link
        self.settings = settings
        self.location = location
        home = HomeModel(link: link)
        desk = DeskModel(link: link, newCommandId: Core.newRequestId)
        clock = ClockModel(link: link, settings: settings)
        let rest = RestModel(settings: settings)
        self.rest = rest
        let buzzer = PhoneBuzzer { rest.vibration }
        self.buzzer = buzzer
        ai = AiModel(link: link, buzzer: buzzer)
        shortcuts = ShortcutModel(link: link, buzzer: buzzer, newCommandId: Core.newRequestId)
        music = MusicModel(link: link, buzzer: buzzer, newCommandId: Core.newRequestId)
        let desk = desk
        ai.onAlert { alert, late in rest.alerted(alert, late: late) }
        ai.onCleared { rest.alertCleared() }
        rest.onResting { [weak self] resting in
            desk.setResting(resting)
            self?.fitScreen()
        }
    }

    @ObservationIgnored private let buzzer: PhoneBuzzer
    @ObservationIgnored private var active = false
    @ObservationIgnored private var battery: [NSObjectProtocol] = []

    /// 쉬는 동안 아래 화면을 트리에서 뺀다(타이머와 애니메이션이 서지 않게, SD-14).
    var showsDesk: Bool { !rest.resting }

    /// 앱이 앞에 있을 때만 화면을 켜 둔다. 배터리는 감시를 켠 동안만 읽힌다(모르면 nil).
    func fitScreen() {
        let device = UIDevice.current
        let charging = device.batteryState == .charging || device.batteryState == .full
        let keep = active && keepScreenOn(rest.keepOn, charging: charging, percent: batteryPercent(device.batteryLevel))
        if UIApplication.shared.isIdleTimerDisabled != keep {
            UIApplication.shared.isIdleTimerDisabled = keep
        }
    }

    private func watchBattery(_ on: Bool) {
        let device = UIDevice.current
        device.isBatteryMonitoringEnabled = on
        battery.forEach(NotificationCenter.default.removeObserver)
        battery = []
        guard on else { return }
        for name in [UIDevice.batteryStateDidChangeNotification, UIDevice.batteryLevelDidChangeNotification] {
            battery.append(NotificationCenter.default.addObserver(forName: name, object: nil, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.fitScreen() }
            })
        }
    }

    func foreground() {
        active = true
        link.start()
        desk.setVisible(true)
        clock.setVisible(true)
        rest.setVisible(true)
        buzzer.start()
        watchBattery(true)
        fitScreen()
        location.refresh()
        if !started {
            started = true
            #if DEBUG
                Demo.apply(self)
            #endif
        }
        offerLocation()
    }

    func background() {
        active = false
        rest.setVisible(false)
        desk.setVisible(false)
        clock.setVisible(false)
        location.stop()
        clock.done()
        buzzer.stop()
        watchBattery(false)
        fitScreen()
        link.stop()
    }

    /// 위치 권한을 아직 정하지 않았고 안내를 보인 적 없으면 띄운다.
    /// 블루투스 권한 창과 겹치지 않게 블루투스 상태를 알게 된 뒤(최대 3초 기다림)에 띄운다.
    private func offerLocation() {
        #if DEBUG
            // 보기용 값으로 찍을 때는 `-locationNotice` 로만 띄운다.
            if Demo.on { return }
        #endif
        guard !offered, location.undecided, !settings.askedLocation else { return }
        offered = true
        Task { [weak self] in
            for _ in 0 ..< 30 {
                if self?.link.feed.bluetoothDecided ?? true { break }
                try? await Task.sleep(for: .milliseconds(100))
            }
            self?.askingLocation = true
        }
    }

    /// 바깥 누르기(nil)는 이번만 닫고, 다음에 켤 때 다시 묻는다.
    func answerLocation(_ next: Bool?) {
        askingLocation = false
        guard let next else { return }
        settings.askedLocation = true
        if next {
            location.ask()
        } else {
            link.setLocation(.off)
        }
    }
}

struct RootView: View {
    let world: World

    var body: some View {
        let theme = world.clock.theme
        let blocking = world.home.pairingCode != nil || world.home.problem != nil
        ZStack {
            if blocking {
                Zoomed { HomeView(model: world.home) }
            } else if world.showsDesk {
                DeskView(
                    desk: world.desk,
                    clock: world.clock,
                    ai: world.ai,
                    shortcuts: world.shortcuts,
                    music: world.music,
                    rest: world.rest,
                    restOpen: world.restOpen,
                    notice: world.home.notice,
                    modal: world.askingLocation ? World.locationNotice : nil,
                    onModal: world.answerLocation
                )
            }
            if world.rest.resting {
                RestScreen(rest: world.rest, clock: world.clock)
            }
        }
        // 배율을 키운 화면 밖(안전 영역)도 바탕색으로 채운다.
        .background(Colors.of(theme).ground.ignoresSafeArea())
        .environment(\.palette, Colors.of(theme))
        // 상태 표시줄을 시스템 설정이 아니라 앱 테마에 맞춘다.
        .preferredColorScheme(theme == .light ? .light : .dark)
        // 누름마다 쉬기 타이머를 다시 잰다. 보기만 하고 아래 누름을 막지 않는다.
        .simultaneousGesture(DragGesture(minimumDistance: 0).onChanged { _ in
            if !world.rest.resting { world.rest.touched() }
        })
        // 쉬지 않게 막는 까닭들
        .onChange(of: world.clock.picking, initial: true) { _, on in world.rest.hold(.picking, on) }
        .onChange(of: world.desk.asking != nil || world.askingLocation, initial: true) { _, on in world.rest.hold(.confirm, on) }
        .onChange(of: blocking, initial: true) { _, on in world.rest.hold(.pairing, on) }
        .onChange(of: world.link.feed.state, initial: true) { _, state in
            if case .connected = state { world.music.connectionChanged(connected: true) } else { world.music.connectionChanged(connected: false) }
        }
        .onChange(of: world.rest.keepOn) { world.fitScreen() }
    }
}
