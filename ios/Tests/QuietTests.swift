import Foundation
import Testing

@testable import SideDesk

/// SD-14 원칙: 값이 그대로면 매 프레임 그리기와 1초 타이머가 없다.
struct QuietTests {
    @Test func clockEntriesComeOnlyAtMinuteBoundaries() {
        let start = Date(timeIntervalSince1970: 1_795_000_037)
        let entries = Array(MinuteSchedule().entries(from: start, mode: .normal).prefix(4))
        #expect(entries[0] == start)
        for pair in zip(entries.dropFirst(), entries.dropFirst(2)) {
            #expect(pair.1.timeIntervalSince(pair.0) == 60)
        }
        #expect(Int(entries[1].timeIntervalSince1970) % 60 == 0, "다음은 분 0초")
        #expect(nextMinute(after: entries[1]) == entries[2])
    }

    @Test func restShiftChangesOnlyWhenTheMinuteDoes() {
        let a = shiftAt(minute: 100)
        let b = shiftAt(minute: 100)
        #expect(a == b)
        #expect(shiftAt(minute: 101) != a)
        #expect(shiftAt(minute: 109) == a, "9자리를 돈다")
        #expect(shiftAt(minute: -1) == shiftAt(minute: 8))
    }

    @Test func wavePhaseDependsOnlyOnTheValue() {
        #expect(wavePhase(40) == wavePhase(40))
        #expect(wavePhase(40) != wavePhase(41))
        #expect(wavePhase(nil) == 0)
        #expect(wavePhase(250) == wavePhase(100))
    }

    @MainActor
    @Test func restingTakesTheDeskOutOfTheTree() {
        let world = World(link: FakeLink(), settings: MemorySettings(), location: FakeLocation())
        #expect(world.showsDesk)
        world.rest.forceRest()
        #expect(!world.showsDesk)
        world.rest.touched()
        #expect(world.showsDesk)
    }
}
