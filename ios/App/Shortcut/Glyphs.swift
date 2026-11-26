import SwiftUI

/// 정해진 그림 묶음(be, 안드로이드와 같은 이름표).
let glyphNames: Set<String> = [
    "terminal", "folder", "code", "browser", "calculator", "settings", "activity",
    "music", "volume", "dnd", "lock", "file", "star", "bolt",
]

/// 색 이름표의 실제 색. 안드로이드 `shortcutColor` 와 같다. 모르면 회색
func shortcutColor(_ name: String) -> Color {
    switch name {
    case "slate": Color(hex: 0x4F7090)
    case "green": Color(hex: 0x6C8A6E)
    case "purple": Color(hex: 0x8A7AA8)
    case "orange": Color(hex: 0xA8805A)
    case "teal": Color(hex: 0x5E8A94)
    case "olive": Color(hex: 0x7A8F5E)
    case "rose": Color(hex: 0xA86A7C)
    case "sand": Color(hex: 0xE0B95C)
    case "sky": Color(hex: 0x7FA2D9)
    default: Color(hex: 0x747C88)
    }
}

/// 색 이름표의 밝기(0~1). 켜진 칸의 그림 색을 테마가 아니라 바탕 밝기로 정한다.
func shortcutLuminance(_ name: String) -> Double {
    let hex: UInt32 = switch name {
    case "slate": 0x4F7090
    case "green": 0x6C8A6E
    case "purple": 0x8A7AA8
    case "orange": 0xA8805A
    case "teal": 0x5E8A94
    case "olive": 0x7A8F5E
    case "rose": 0xA86A7C
    case "sand": 0xE0B95C
    case "sky": 0x7FA2D9
    default: 0x747C88
    }
    func channel(_ value: UInt32) -> Double {
        let c = Double(value & 0xFF) / 255
        return c <= 0.03928 ? c / 12.92 : pow((c + 0.055) / 1.055, 2.4)
    }
    return 0.2126 * channel(hex >> 16) + 0.7152 * channel(hex >> 8) + 0.0722 * channel(hex)
}

/// 켜진 칸의 그림 색. 노란 바탕이면 두 테마 모두 어두운 그림
func onColor(_ name: String) -> Color {
    shortcutLuminance(name) > 0.4 ? Color(hex: 0x1A1D22) : .white
}

/// 단축키 그림. 선으로만 그린다. 모르는 이름표는 이름 첫 글자다.
struct Glyph: View {
    let icon: String
    let fallback: String
    let color: Color

    var body: some View {
        if glyphNames.contains(icon) {
            Canvas { context, size in
                Glyphs.draw(icon, color: color, in: &context, size: size)
            }
        } else {
            GeometryReader { proxy in
                Text(fallback.first.map(String.init) ?? "?")
                    .font(.system(size: proxy.size.height * 0.7, weight: .semibold))
                    .foregroundStyle(color)
                    .frame(width: proxy.size.width, height: proxy.size.height)
            }
        }
    }
}

/// 초승달: 원에서 오른쪽 위로 비킨 원을 뺀 모양
func crescent(center: CGPoint, radius r: Double) -> Path {
    let disc = Path(ellipseIn: CGRect(x: center.x - r, y: center.y - r, width: r * 2, height: r * 2))
    let bite = CGRect(x: center.x + r * 0.5 - r * 0.85, y: center.y - r * 0.35 - r * 0.85, width: r * 1.7, height: r * 1.7)
    return disc.subtracting(Path(ellipseIn: bite))
}

enum Glyphs {
    static func draw(_ icon: String, color: Color, in context: inout GraphicsContext, size: CGSize) {
        let w = size.width
        let h = size.height
        let m = min(w, h)
        let width = m * 0.08
        let style = StrokeStyle(lineWidth: width, lineCap: .round, lineJoin: .round)
        let ink = GraphicsContext.Shading.color(color)
        func p(_ x: Double, _ y: Double) -> CGPoint { CGPoint(x: w * x, y: h * y) }
        func line(_ points: [(Double, Double)], close: Bool = false) {
            var path = Path()
            path.addLines(points.map { p($0.0, $0.1) })
            if close { path.closeSubpath() }
            context.stroke(path, with: ink, style: style)
        }
        func box(_ left: Double, _ top: Double, _ right: Double, _ bottom: Double, round: Double = 0.1) {
            let rect = CGRect(x: w * left, y: h * top, width: w * (right - left), height: h * (bottom - top))
            context.stroke(Path(roundedRect: rect, cornerRadius: m * round), with: ink, style: style)
        }
        func dot(_ center: CGPoint, _ r: Double, filled: Bool = true) {
            let path = Path(ellipseIn: CGRect(x: center.x - r, y: center.y - r, width: r * 2, height: r * 2))
            if filled { context.fill(path, with: ink) } else { context.stroke(path, with: ink, style: style) }
        }
        /// Compose `drawArc` 와 같은 뜻: 사각형 안 타원의 시작 각도에서 시계 방향으로 sweep 도
        func arc(_ left: Double, _ top: Double, _ arcWidth: Double, _ arcHeight: Double, start: Double, sweep: Double) {
            let cx = w * left + w * arcWidth / 2
            let cy = h * top + h * arcHeight / 2
            let rx = w * arcWidth / 2
            let ry = h * arcHeight / 2
            var path = Path()
            for step in 0 ... 24 {
                let a = (start + sweep * Double(step) / 24) * .pi / 180
                let point = CGPoint(x: cx + rx * cos(a), y: cy + ry * sin(a))
                if step == 0 { path.move(to: point) } else { path.addLine(to: point) }
            }
            context.stroke(path, with: ink, style: style)
        }

        switch icon {
        case "terminal":
            line([(0.2, 0.3), (0.42, 0.5), (0.2, 0.7)])
            line([(0.5, 0.72), (0.8, 0.72)])
        case "folder":
            line([(0.14, 0.3), (0.4, 0.3), (0.48, 0.38), (0.86, 0.38), (0.86, 0.76), (0.14, 0.76)], close: true)
        case "code":
            line([(0.36, 0.3), (0.16, 0.5), (0.36, 0.7)])
            line([(0.64, 0.3), (0.84, 0.5), (0.64, 0.7)])
        case "browser":
            let c = p(0.5, 0.5)
            let r = m * 0.34
            dot(c, r, filled: false)
            context.stroke(Path(ellipseIn: CGRect(x: c.x - r * 0.45, y: c.y - r, width: r * 0.9, height: r * 2)), with: ink, style: style)
            var across = Path()
            across.move(to: CGPoint(x: c.x - r, y: c.y))
            across.addLine(to: CGPoint(x: c.x + r, y: c.y))
            context.stroke(across, with: ink, style: style)
        case "calculator":
            box(0.24, 0.14, 0.76, 0.86)
            box(0.34, 0.24, 0.66, 0.38, round: 0.03)
            for row in 0 ... 1 {
                for col in 0 ... 1 {
                    dot(p(0.4 + Double(col) * 0.2, 0.54 + Double(row) * 0.18), width * 0.7)
                }
            }
        case "settings":
            for (i, knob) in [0.35, 0.65, 0.45].enumerated() {
                let y = 0.3 + Double(i) * 0.2
                line([(0.16, y), (0.84, y)])
                dot(p(knob, y), width * 1.3)
            }
        case "activity":
            line([(0.12, 0.52), (0.32, 0.52), (0.42, 0.3), (0.56, 0.72), (0.66, 0.52), (0.88, 0.52)])
        case "music":
            line([(0.4, 0.66), (0.4, 0.24), (0.76, 0.18), (0.76, 0.6)])
            dot(p(0.32, 0.68), m * 0.09, filled: false)
            dot(p(0.68, 0.62), m * 0.09, filled: false)
        case "volume":
            line([(0.16, 0.4), (0.3, 0.4), (0.48, 0.24), (0.48, 0.76), (0.3, 0.6), (0.16, 0.6)], close: true)
            arc(0.48, 0.36, 0.2, 0.28, start: -45, sweep: 90)
            arc(0.46, 0.24, 0.36, 0.52, start: -50, sweep: 100)
        case "dnd":
            context.stroke(crescent(center: p(0.5, 0.5), radius: m * 0.3), with: ink, style: style)
        case "lock":
            box(0.24, 0.44, 0.76, 0.84)
            arc(0.33, 0.18, 0.34, 0.5, start: 180, sweep: 180)
            line([(0.33, 0.43), (0.33, 0.44)])
            line([(0.67, 0.43), (0.67, 0.44)])
        case "file":
            line([(0.26, 0.14), (0.58, 0.14), (0.76, 0.32), (0.76, 0.86), (0.26, 0.86)], close: true)
        case "star":
            let points = (0 ..< 10).map { i -> (Double, Double) in
                let r = i % 2 == 0 ? 0.36 : 0.15
                let a = -Double.pi / 2 + Double(i) * .pi / 5
                return (0.5 + cos(a) * r, 0.53 + sin(a) * r)
            }
            line(points, close: true)
        case "bolt":
            line([(0.56, 0.12), (0.28, 0.54), (0.5, 0.54), (0.44, 0.88), (0.72, 0.44), (0.5, 0.44)], close: true)
        default:
            break
        }
    }
}
