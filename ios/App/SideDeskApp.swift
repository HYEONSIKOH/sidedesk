import SwiftUI

@main
struct SideDeskApp: App {
    @Environment(\.scenePhase) private var phase
    @State private var home = HomeModel(link: makeLink())

    var body: some Scene {
        WindowGroup {
            HomeView(model: home)
        }
        .onChange(of: phase, initial: true) { _, phase in
            switch phase {
            case .active: home.start()
            case .background: home.stop()
            default: break
            }
        }
    }
}
