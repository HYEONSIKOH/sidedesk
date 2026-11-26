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
    /// 위치 권한을 묻기 전에 한 번 보이는 안내
    var askingLocation = false

    @ObservationIgnored private let settings: any Settings
    @ObservationIgnored private let location: CoarseLocation
    @ObservationIgnored private var offered = false
    @ObservationIgnored private var started = false

    init() {
        let settings = DefaultsSettings()
        #if DEBUG
            let link: any ComputerLink = Demo.on ? DemoLink() : PhoneLink()
        #else
            let link: any ComputerLink = PhoneLink()
        #endif
        self.link = link
        self.settings = settings
        home = HomeModel(link: link)
        desk = DeskModel(link: link, newCommandId: Core.newRequestId)
        clock = ClockModel(link: link, settings: settings)
        location = CoarseLocation(link: link)
    }

    func foreground() {
        link.start()
        desk.setVisible(true)
        clock.setVisible(true)
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
        desk.setVisible(false)
        clock.setVisible(false)
        location.stop()
        clock.done()
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
        Group {
            if world.home.pairingCode != nil || world.home.problem != nil {
                HomeView(model: world.home)
            } else {
                DeskView(
                    desk: world.desk,
                    clock: world.clock,
                    notice: world.home.notice,
                    modal: world.askingLocation ? World.locationNotice : nil,
                    onModal: world.answerLocation
                )
            }
        }
        .environment(\.palette, Colors.of(theme))
        // 상태 표시줄을 시스템 설정이 아니라 앱 테마에 맞춘다.
        .preferredColorScheme(theme == .light ? .light : .dark)
    }
}
