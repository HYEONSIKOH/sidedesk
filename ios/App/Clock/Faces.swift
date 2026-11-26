import SwiftUI

private let days = ["일", "월", "화", "수", "목", "금", "토"]

/// 기기 시각을 나눈 값. 시계는 컴퓨터와 끊겨도 기기 시각으로 간다.
struct ClockTime {
    let hour: Int
    let minute: Int
    let second: Int
    let month: Int
    let day: Int
    /// 0 이 일요일
    let weekday: Int
    /// 이번 달 날 수, 1일의 요일
    let daysInMonth: Int
    let firstWeekday: Int

    init(_ date: Date, calendar: Calendar = .current) {
        let parts = calendar.dateComponents([.hour, .minute, .second, .month, .day, .weekday], from: date)
        hour = parts.hour ?? 0
        minute = parts.minute ?? 0
        second = parts.second ?? 0
        month = parts.month ?? 1
        day = parts.day ?? 1
        weekday = (parts.weekday ?? 1) - 1
        daysInMonth = calendar.range(of: .day, in: .month, for: date)?.count ?? 30
        let first = calendar.date(from: calendar.dateComponents([.year, .month], from: date)) ?? date
        firstWeekday = calendar.component(.weekday, from: first) - 1
    }

    var timeText: String { String(format: "%02d:%02d", hour, minute) }
    var dateText: String { "\(month)월 \(day)일 \(days[weekday])요일" }
    var dayName: String { days[weekday] }
}

func degrees(_ value: Float) -> String {
    "\(Int(value.rounded()))°"
}

/// 동그라미가 기준으로 삼을 묶음(숫자 시각, 큰 기온, 큰 날짜). 모양마다 하나만 붙인다.
private struct SunAnchorKey: PreferenceKey {
    static let defaultValue: Anchor<CGRect>? = nil
    static func reduce(value: inout Anchor<CGRect>?, nextValue: () -> Anchor<CGRect>?) {
        value = value ?? nextValue()
    }
}

extension View {
    fileprivate func sunAnchor() -> some View {
        anchorPreference(key: SunAnchorKey.self, value: .bounds) { $0 }
    }
}

/// 홈 큰 화면. 고른 모양 하나와, 그 모양의 기준 묶음 뒤에서 시각 따라 움직이는 동그라미
struct HomeBig: View {
    let clock: ClockModel
    let now: Date
    let openCredit: () -> Void

    var body: some View {
        GeometryReader { proxy in
            let w = proxy.size.width
            let h = proxy.size.height
            Group {
                switch clock.face {
                case .digits: DigitsFace(clock: clock, now: now, w: w, h: h, openCredit: openCredit)
                case .dial: DialFace(clock: clock, now: now, w: w, h: h, openCredit: openCredit)
                case .weather: WeatherFace(clock: clock, now: now, w: w, h: h, openCredit: openCredit)
                case .calendar: CalendarFace(now: now, w: w, h: h)
                case .overview: OverviewFace(clock: clock, now: now, openCredit: openCredit)
                }
            }
            .frame(width: w, height: h)
            .backgroundPreferenceValue(SunAnchorKey.self) { anchor in
                GeometryReader { inner in
                    if let anchor {
                        SunCircle(now: now, anchor: inner[anchor])
                    }
                }
            }
        }
    }
}

/// 기준 묶음 안에서 움직이는 동그라미. 낮(6~19시)과 밤(19~6시)을 각각 왼쪽에서 오른쪽으로 반원을 그리며 지난다.
private struct SunCircle: View {
    let now: Date
    let anchor: CGRect
    @Environment(\.palette) private var palette

    var body: some View {
        let time = ClockTime(now)
        let hour = Double(time.hour) + Double(time.minute) / 60
        let u = hour >= 6 && hour < 19 ? (hour - 6) / 13 : (hour - 19 + 24).truncatingRemainder(dividingBy: 24) / 11
        let r = anchor.height * 0.62
        let x = anchor.minX + anchor.width * (0.08 + 0.84 * u)
        let y = anchor.minY + anchor.height * (0.86 - 0.6 * sin(.pi * u))
        Circle()
            .fill(palette.card)
            .frame(width: r * 2, height: r * 2)
            .position(x: x, y: y)
    }
}

private struct DateLine: View {
    let clock: ClockModel
    let now: Date
    let size: Double
    @Environment(\.palette) private var palette

    var body: some View {
        HStack(spacing: 8) {
            Text(ClockTime(now).dateText)
                .font(.system(size: size, weight: .semibold))
                .foregroundStyle(palette.ink)
                .lineLimit(1)
            if let via = clock.via {
                ViaIcon(via: via, color: palette.muted).frame(width: size, height: size)
            }
        }
    }
}

/// 시계 숫자. 둥근 가는 글꼴에 위에서 아래로 옅어지는 색
struct Digits: View {
    let text: String
    let size: Double
    @Environment(\.palette) private var palette

    var body: some View {
        Text(text)
            .font(.system(size: size, weight: .thin, design: .rounded))
            .tracking(size * 0.04)
            .monospacedDigit()
            .foregroundStyle(LinearGradient(colors: [palette.digitTop, palette.digitBottom], startPoint: .top, endPoint: .bottom))
            .lineLimit(1)
            .fixedSize()
    }
}

/// 하늘 아이콘, 기온, 최고와 최저. 받은 적 없으면 --°
private struct WeatherLine: View {
    let clock: ClockModel
    let size: Double
    @Environment(\.palette) private var palette

    var body: some View {
        HStack(spacing: 0) {
            if let weather = clock.weather {
                SkyIcon(sky: weather.sky, isDay: weather.isDay, color: palette.ink)
                    .frame(width: size * 1.3, height: size * 1.3)
                    .padding(.trailing, 6)
            }
            Text(clock.temperature)
                .font(.system(size: size * 1.15, weight: .medium))
                .foregroundStyle(palette.ink)
            if let weather = clock.weather {
                Text("  최고 \(degrees(weather.high)) 최저 \(degrees(weather.low))")
                    .font(.system(size: size * 0.9))
                    .foregroundStyle(palette.muted)
            }
        }
        .lineLimit(1)
        .dim(clock.weatherDimmed)
    }
}

/// 날씨 출처. Open-Meteo 는 누르면 누리집을 연다.
struct Credit: View {
    let clock: ClockModel
    var size: Double = 11
    let open: () -> Void
    @Environment(\.palette) private var palette

    var body: some View {
        if let credit = clock.credit {
            Text(credit)
                .font(.system(size: size))
                .underline(clock.weather?.source == .openMeteo)
                .foregroundStyle(palette.muted)
                // 출처 글자는 잘리면 안 되므로 줄임표 대신 두 줄까지 쓴다.
                .lineLimit(2)
                .multilineTextAlignment(.center)
                .dim(clock.weatherDimmed)
                .onTapGesture {
                    if clock.weather?.source == .openMeteo { open() }
                }
        }
    }
}

private struct DigitsFace: View {
    let clock: ClockModel
    let now: Date
    let w: Double
    let h: Double
    let openCredit: () -> Void

    var body: some View {
        let digit = min(w * 0.27, h * 0.4)
        let line = min(max(min(w * 0.045, h * 0.06), 13), 26)
        VStack(spacing: 0) {
            DateLine(clock: clock, now: now, size: line)
            Digits(text: ClockTime(now).timeText, size: digit).sunAnchor()
            WeatherLine(clock: clock, size: line)
            Credit(clock: clock, open: openCredit).padding(.top, 6)
        }
        .padding(16)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

private struct DialFace: View {
    let clock: ClockModel
    let now: Date
    let w: Double
    let h: Double
    let openCredit: () -> Void
    @Environment(\.palette) private var palette

    var body: some View {
        let wide = w > h
        let dial = min(wide ? w * 0.45 : w * 0.7, h * (wide ? 0.85 : 0.5))
        let line = min(max(min(w * 0.045, h * 0.06), 13), 24)
        let alignment: HorizontalAlignment = wide ? .leading : .center
        let info = VStack(alignment: alignment, spacing: 0) {
            DateLine(clock: clock, now: now, size: line)
            VStack(alignment: alignment, spacing: 0) {
                Text(clock.temperature)
                    .font(.system(size: line * 2.4, weight: .medium))
                    .foregroundStyle(palette.ink)
                    .sunAnchor()
                if let weather = clock.weather {
                    Text(skyName(weather.sky)).font(.system(size: line)).foregroundStyle(palette.muted)
                    Text("최고 \(degrees(weather.high)) 최저 \(degrees(weather.low))").font(.system(size: line)).foregroundStyle(palette.muted)
                }
            }
            .lineLimit(1)
            .dim(clock.weatherDimmed)
            Credit(clock: clock, open: openCredit).padding(.top, 6)
        }
        Group {
            if wide {
                HStack(spacing: 28) {
                    Dial(now: now).frame(width: dial, height: dial)
                    info
                }
            } else {
                VStack(spacing: 20) {
                    Dial(now: now).frame(width: dial, height: dial)
                    info
                }
            }
        }
        .padding(16)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

/// 바늘 시계. 60 눈금과 12, 3, 6, 9
struct Dial: View {
    let now: Date
    @Environment(\.palette) private var palette

    var body: some View {
        let time = ClockTime(now)
        let minute = Double(time.minute) + Double(time.second) / 60
        let hour = Double(time.hour % 12) + minute / 60
        GeometryReader { proxy in
            let r = min(proxy.size.width, proxy.size.height) / 2
            let center = CGPoint(x: proxy.size.width / 2, y: proxy.size.height / 2)
            ZStack {
                Canvas { context, _ in
                    for i in 0 ..< 60 {
                        let a = Double(i) * .pi / 30
                        let long = i % 5 == 0
                        let from = r * (long ? 0.84 : 0.89)
                        let to = r * 0.95
                        var path = Path()
                        path.move(to: CGPoint(x: center.x + sin(a) * from, y: center.y - cos(a) * from))
                        path.addLine(to: CGPoint(x: center.x + sin(a) * to, y: center.y - cos(a) * to))
                        context.stroke(path, with: .color(long ? palette.ink : palette.muted), style: StrokeStyle(lineWidth: r * (long ? 0.025 : 0.012), lineCap: .round))
                    }
                    func hand(_ turn: Double, _ length: Double, _ width: Double, _ color: Color) {
                        let a = turn * 2 * .pi
                        var path = Path()
                        path.move(to: CGPoint(x: center.x - sin(a) * r * 0.08, y: center.y + cos(a) * r * 0.08))
                        path.addLine(to: CGPoint(x: center.x + sin(a) * r * length, y: center.y - cos(a) * r * length))
                        context.stroke(path, with: .color(color), style: StrokeStyle(lineWidth: r * width, lineCap: .round))
                    }
                    hand(hour / 12, 0.5, 0.06, palette.ink)
                    hand(minute / 60, 0.78, 0.04, palette.accent)
                    let dot = r * 0.05
                    context.fill(Path(ellipseIn: CGRect(x: center.x - dot, y: center.y - dot, width: dot * 2, height: dot * 2)), with: .color(palette.ink))
                }
                ForEach([12, 3, 6, 9], id: \.self) { n in
                    let a = Double(n) * .pi / 6
                    Text("\(n)")
                        .font(.system(size: r * 0.16))
                        .foregroundStyle(palette.ink)
                        .position(x: center.x + sin(a) * r * 0.68, y: center.y - cos(a) * r * 0.68)
                }
            }
        }
        .aspectRatio(1, contentMode: .fit)
    }
}

private struct WeatherFace: View {
    let clock: ClockModel
    let now: Date
    let w: Double
    let h: Double
    let openCredit: () -> Void
    @Environment(\.palette) private var palette

    var body: some View {
        let big = min(w * 0.2, h * 0.3)
        let line = min(max(min(w * 0.045, h * 0.06), 13), 24)
        VStack(spacing: 0) {
            HStack(spacing: 16) {
                Text(clock.temperature)
                    .font(.system(size: big, weight: .light))
                    .foregroundStyle(palette.ink)
                    .sunAnchor()
                if let weather = clock.weather {
                    VStack(alignment: .leading, spacing: 0) {
                        SkyIcon(sky: weather.sky, isDay: weather.isDay, color: palette.ink).frame(width: line * 2.6, height: line * 2.6)
                        Text(skyName(weather.sky)).font(.system(size: line)).foregroundStyle(palette.ink)
                        Text("최고 \(degrees(weather.high)) 최저 \(degrees(weather.low))").font(.system(size: line * 0.9)).foregroundStyle(palette.muted)
                    }
                }
            }
            .lineLimit(1)
            .dim(clock.weatherDimmed)
            Credit(clock: clock, open: openCredit).padding(.top, 4)
            Spacer().frame(height: 12)
            DateLine(clock: clock, now: now, size: line)
            Digits(text: ClockTime(now).timeText, size: line * 3)
        }
        .padding(16)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

private struct CalendarFace: View {
    let now: Date
    let w: Double
    let h: Double
    @Environment(\.palette) private var palette

    var body: some View {
        let time = ClockTime(now)
        let wide = w > h
        let line = min(max(min(w * 0.04, h * 0.055), 12), 22)
        let left = VStack(alignment: wide ? .leading : .center, spacing: 0) {
            Text("\(time.month)월 \(time.dayName)요일").font(.system(size: line)).foregroundStyle(palette.muted)
            Text("\(time.day)").font(.system(size: line * 5, weight: .light)).foregroundStyle(palette.ink).sunAnchor()
            Digits(text: time.timeText, size: line * 2.2)
        }
        .lineLimit(1)
        Group {
            if wide {
                HStack(spacing: 0) {
                    left.frame(width: (w - 40) * 0.4, alignment: .leading)
                    Month(time: time, size: line).frame(width: (w - 40) * 0.6)
                }
            } else {
                VStack(spacing: 16) {
                    left
                    Month(time: time, size: line)
                }
            }
        }
        .padding(20)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

/// 이번 달 달력. 오늘은 동그라미
private struct Month: View {
    let time: ClockTime
    let size: Double
    @Environment(\.palette) private var palette

    var body: some View {
        let cells = Array(repeating: 0, count: time.firstWeekday) + Array(1 ... time.daysInMonth)
        let weeks = stride(from: 0, to: cells.count, by: 7).map { Array(cells[$0 ..< min($0 + 7, cells.count)]) }
        VStack(spacing: 4) {
            HStack(spacing: 0) {
                ForEach(days, id: \.self) { day in
                    Text(day).font(.system(size: size * 0.85)).foregroundStyle(palette.muted).frame(maxWidth: .infinity)
                }
            }
            ForEach(weeks.indices, id: \.self) { index in
                HStack(spacing: 0) {
                    ForEach(0 ..< 7, id: \.self) { column in
                        let day = column < weeks[index].count ? weeks[index][column] : 0
                        Group {
                            if day > 0 {
                                let today = day == time.day
                                Text("\(day)")
                                    .font(.system(size: size * 0.9))
                                    .foregroundStyle(today ? palette.panel : palette.ink)
                                    .padding(.horizontal, 6)
                                    .padding(.vertical, 2)
                                    .background(Capsule().fill(today ? palette.accent : .clear))
                            } else {
                                Color.clear.frame(height: 1)
                            }
                        }
                        .frame(maxWidth: .infinity)
                    }
                }
            }
        }
    }
}

/// 칸 넷을 4x1, 2x2, 1x4 가운데 칸이 가장 덜 길쭉해지는 배치로 놓는다.
func overviewGrid(width: Double, height: Double, gap: Double) -> (columns: Int, rows: Int) {
    [(4, 1), (2, 2), (1, 4)].min { a, b in
        func shape(_ grid: (Int, Int)) -> Double {
            let w = (width - gap * Double(grid.0 - 1)) / Double(grid.0)
            let h = (height - gap * Double(grid.1 - 1)) / Double(grid.1)
            return abs(log(w / h))
        }
        return shape(a) < shape(b)
    }.map { (columns: $0.0, rows: $0.1) } ?? (2, 2)
}

/// 모아 보기: 홈의 것만 칸 넷(바늘, 날씨, 날짜, 숫자 시각)으로 모은다.
private struct OverviewFace: View {
    let clock: ClockModel
    let now: Date
    let openCredit: () -> Void
    @Environment(\.palette) private var palette

    var body: some View {
        GeometryReader { proxy in
            let gap = 10.0
            let grid = overviewGrid(width: proxy.size.width, height: proxy.size.height, gap: gap)
            let cellW = (proxy.size.width - gap * Double(grid.columns - 1)) / Double(grid.columns)
            let cellH = (proxy.size.height - gap * Double(grid.rows - 1)) / Double(grid.rows)
            let line = min(max(min(cellW * 0.08, cellH * 0.1), 11), 20)
            let time = ClockTime(now)
            VStack(spacing: gap) {
                ForEach(0 ..< grid.rows, id: \.self) { row in
                    HStack(spacing: gap) {
                        ForEach(0 ..< grid.columns, id: \.self) { column in
                            cell(row * grid.columns + column, time: time, line: line, cellW: cellW, cellH: cellH)
                                .frame(width: cellW, height: cellH)
                                .background(RoundedRectangle(cornerRadius: 22).fill(palette.card))
                        }
                    }
                }
            }
        }
        .padding(4)
    }

    @ViewBuilder
    private func cell(_ index: Int, time: ClockTime, line: Double, cellW: Double, cellH: Double) -> some View {
        switch index {
        case 0:
            Dial(now: now).padding(12)
        case 1:
            VStack(spacing: 0) {
                VStack(spacing: 0) {
                    if let weather = clock.weather {
                        SkyIcon(sky: weather.sky, isDay: weather.isDay, color: palette.ink).frame(width: line * 2, height: line * 2)
                    }
                    Text(clock.temperature).font(.system(size: line * 2.4, weight: .medium)).foregroundStyle(palette.ink)
                    if let weather = clock.weather {
                        Text("최고 \(degrees(weather.high)) 최저 \(degrees(weather.low))").font(.system(size: line * 0.9)).foregroundStyle(palette.muted)
                    }
                }
                .lineLimit(1)
                .dim(clock.weatherDimmed)
                Credit(clock: clock, size: line * 0.7, open: openCredit).padding(.top, 4)
            }
            .padding(14)
        case 2:
            VStack(spacing: 0) {
                Text("\(time.month)월").font(.system(size: line)).foregroundStyle(palette.muted)
                Text("\(time.day)").font(.system(size: line * 3.4, weight: .light)).foregroundStyle(palette.ink)
                Text("\(time.dayName)요일").font(.system(size: line)).foregroundStyle(palette.ink)
            }
            .lineLimit(1)
            .padding(14)
        default:
            VStack(spacing: 6) {
                Digits(text: time.timeText, size: min(cellW * 0.22, cellH * 0.35))
                if let via = clock.via {
                    ViaIcon(via: via, color: palette.muted).frame(width: line, height: line)
                }
            }
            .padding(14)
        }
    }
}

/// 다른 화면을 볼 때의 홈 작은 칸. 시각, 하늘 아이콘, 기온, 받은 곳
struct HomeTile: View {
    let clock: ClockModel
    let now: Date
    @Environment(\.palette) private var palette

    var body: some View {
        ZStack {
            HStack(spacing: 6) {
                Text(ClockTime(now).timeText)
                    .font(.system(size: 24, weight: .medium))
                    .foregroundStyle(palette.ink)
                VStack(spacing: 0) {
                    if let weather = clock.weather {
                        SkyIcon(sky: weather.sky, isDay: weather.isDay, color: palette.ink).frame(width: 14, height: 14)
                    }
                    Text(clock.temperature)
                        .font(.system(size: 11))
                        .foregroundStyle(palette.ink)
                }
                .dim(clock.weatherDimmed)
            }
            .lineLimit(1)
            if let credit = clock.shortCredit {
                VStack {
                    Spacer()
                    Text(credit).font(.system(size: 7)).foregroundStyle(palette.muted).dim(clock.weatherDimmed)
                }
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 6)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(RoundedRectangle(cornerRadius: 20).fill(palette.side))
    }
}

/// 길게 누르면 뜨는 고르기 줄. 모양 다섯, 테마 둘, 완료
struct FacePicker: View {
    let clock: ClockModel
    @Environment(\.palette) private var palette

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 6) {
                    ForEach(Face.allCases, id: \.self) { face in
                        chip(face.label, face == clock.face) { clock.setFace(face) }
                    }
                }
            }
            HStack(spacing: 6) {
                chip("라이트", clock.theme == .light) { clock.setTheme(.light) }
                chip("다크", clock.theme == .dark) { clock.setTheme(.dark) }
                Spacer()
                Button(action: clock.done) {
                    Text("완료")
                        .font(.system(size: 14, weight: .semibold))
                        .foregroundStyle(palette.panel)
                        .padding(.horizontal, 20)
                        .padding(.vertical, 9)
                        .background(Capsule().fill(palette.accent))
                }
                .buttonStyle(.plain)
            }
        }
        .padding(10)
        .background(RoundedRectangle(cornerRadius: 22).fill(palette.raised))
    }

    private func chip(_ label: String, _ on: Bool, _ action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Text(label)
                .font(.system(size: 13, weight: on ? .semibold : .regular))
                .foregroundStyle(on ? palette.panel : palette.ink)
                .lineLimit(1)
                .padding(.horizontal, 14)
                .padding(.vertical, 8)
                .background(Capsule().fill(on ? palette.accent : palette.card))
        }
        .buttonStyle(.plain)
    }
}
