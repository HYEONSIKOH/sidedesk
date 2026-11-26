import Foundation
import SideDeskBridge
import Testing

@testable import SideDesk

struct BridgeTests {
    @Test func unknownEventIsIgnored() {
        // SD-7 이 더한 일처럼 iOS 화면이 아직 없는 일은 버린다.
        #expect(BridgeSession.map(.forge(github: nil, gitlab: nil)) == nil)
        #expect(BridgeSession.map(.computerName(name: "맥")) == .computerName("맥"))
    }
}
