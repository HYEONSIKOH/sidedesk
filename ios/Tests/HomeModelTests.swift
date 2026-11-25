import Observation
import Testing

@testable import SideDesk

@MainActor
struct HomeModelTests {
    @Observable
    final class FakeLink: ComputerLink {
        var state = LinkState.waiting

        func start() {}
        func stop() {}
    }

    @Test func startsByWaiting() {
        let model = HomeModel(link: FakeLink())
        #expect(model.computerName == nil)
        #expect(model.problem == nil)
    }

    @Test func showsTheLatestName() {
        let link = FakeLink()
        let model = HomeModel(link: link)
        link.state = .received("a")
        link.state = .received("b")
        #expect(model.computerName == "b")
    }

    @Test func showsAFailureAndRecovers() {
        let link = FakeLink()
        let model = HomeModel(link: link)
        link.state = .failed("열지 못함")
        #expect(model.problem == "열지 못함")
        link.state = .waiting
        #expect(model.problem == nil)
    }
}
