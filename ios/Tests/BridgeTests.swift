import Foundation
import SideDeskBridge
import Testing

@testable import SideDesk

struct BridgeTests {
    @Test func coreEventsBecomeAppValues() {
        #expect(BridgeSession.map(.computerName(name: "맥")) == .computerName("맥"))
        #expect(BridgeSession.map(.forge(github: nil, gitlab: nil)) == .forge(Forge(github: .unknown, gitlab: .unknown)), "받은 적 없는 쪽은 모름")
        let alert = AiAlertInfo(id: 3, sessionId: "s", title: "t", kind: .waiting, at: 9)
        #expect(BridgeSession.map(.aiAlert(value: alert)) == .aiAlert(AiAlert(id: 3, sessionId: "s", title: "t", kind: .waiting, at: 9)))
        let item = ShortcutInfo(id: 2, name: "소리", icon: "volume", color: "sky", toggle: .on, system: true)
        #expect(BridgeSession.map(.shortcuts(book: 7, items: [item])) == .shortcuts(book: 7, items: [Shortcut(id: 2, name: "소리", icon: "volume", color: "sky", toggle: .on, system: true)]))
        #expect(BridgeSession.map(.volume(value: VolumeInfo(available: true, level: 40, muted: true, settable: false))) == .volume(Volume(available: true, level: 40, muted: true, settable: false)))
    }
}
