import SwiftUI

/// 다음 분 0초. 시계는 분이 바뀔 때만 다시 읽는다(SD-14: 1초 타이머 없음).
func nextMinute(after date: Date, calendar: Calendar = ClockTime.calendar) -> Date {
    let start = calendar.dateInterval(of: .minute, for: date)?.start ?? date
    return start.addingTimeInterval(60)
}

/// 분 경계마다 한 번. 시험에서는 `entries` 를 직접 본다.
struct MinuteSchedule: TimelineSchedule {
    func entries(from startDate: Date, mode: TimelineScheduleMode) -> AnyIterator<Date> {
        var next: Date? = startDate
        return AnyIterator {
            defer { next = next.map { nextMinute(after: $0) } }
            return next
        }
    }
}

/// 음악 진행 바만 쓴다. 재생 중이고 보일 때만 이 시계를 붙인다.
struct MusicTicks: TimelineSchedule {
    func entries(from startDate: Date, mode: TimelineScheduleMode) -> AnyIterator<Date> {
        var next = startDate
        return AnyIterator {
            defer { next = next.addingTimeInterval(0.5) }
            return next
        }
    }
}

/// 물결 위상. 값에서 정해지고 시간에 따라 움직이지 않는다(SD-14: 물결은 값이 바뀔 때만).
func wavePhase(_ value: Float?) -> Double {
    let level = Double(min(max(value ?? 0, 0), 100)) / 100
    return level * 2 * .pi
}

/// 값만큼 아래에서 차오르는 물결. 높이만 애니메이션으로 옮긴다.
struct WaveShape: Shape {
    var level: Double
    var phase: Double

    var animatableData: AnimatablePair<Double, Double> {
        get { AnimatablePair(level, phase) }
        set {
            level = newValue.first
            phase = newValue.second
        }
    }

    func path(in rect: CGRect) -> Path {
        var path = Path()
        guard level > 0 else { return path }
        let top = rect.height * (1 - level)
        let amp = 4.0
        path.move(to: CGPoint(x: 0, y: rect.height))
        var x = 0.0
        while x <= rect.width {
            path.addLine(to: CGPoint(x: x, y: top + amp * sin(x / rect.width * 2 * .pi + phase)))
            x += 4
        }
        path.addLine(to: CGPoint(x: rect.width, y: top + amp * sin(2 * .pi + phase)))
        path.addLine(to: CGPoint(x: rect.width, y: rect.height))
        path.closeSubpath()
        return path
    }
}

/// 성능 칸과 CPU 작은 칸의 물결
struct Liquid: View {
    let value: Float?
    @Environment(\.palette) private var palette

    var body: some View {
        let level = Double(min(max(value.flatMap { $0.isFinite ? $0 : nil } ?? 0, 0), 100)) / 100
        WaveShape(level: level, phase: wavePhase(value.flatMap { $0.isFinite ? $0 : nil }))
            .fill(palette.liquid)
            .animation(.easeOut(duration: 0.6), value: level)
    }
}
