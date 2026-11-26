import SwiftUI

func skyName(_ sky: Sky) -> String {
    switch sky {
    case .unknown: ""
    case .clear: "맑음"
    case .partlyCloudy: "구름 조금"
    case .cloudy: "흐림"
    case .fog: "안개"
    case .drizzle: "이슬비"
    case .rain: "비"
    case .snow: "눈"
    case .shower: "소나기"
    case .thunderstorm: "뇌우"
    }
}

/// 하늘 아이콘. 선으로만 그린다. 모르면 그리지 않는다(안드로이드 `SkyIcon` 과 같은 모양).
struct SkyIcon: View {
    let sky: Sky
    let isDay: Bool
    let color: Color

    var body: some View {
        Canvas { context, size in
            guard sky != .unknown else { return }
            let w = size.width
            let h = size.height
            let width = min(w, h) * 0.075
            let style = StrokeStyle(lineWidth: width, lineCap: .round, lineJoin: .round)
            func stroke(_ path: Path) {
                context.stroke(path, with: .color(color), style: style)
            }
            func line(_ a: CGPoint, _ b: CGPoint) {
                stroke(Path { $0.move(to: a); $0.addLine(to: b) })
            }
            func sun(_ cx: Double, _ cy: Double, _ r: Double) {
                let center = CGPoint(x: w * cx, y: h * cy)
                let radius = min(w, h) * r
                stroke(Path(ellipseIn: CGRect(x: center.x - radius, y: center.y - radius, width: radius * 2, height: radius * 2)))
                for i in 0 ..< 8 {
                    let a = Double(i) * .pi / 4
                    line(
                        CGPoint(x: center.x + cos(a) * radius * 1.5, y: center.y + sin(a) * radius * 1.5),
                        CGPoint(x: center.x + cos(a) * radius * 2, y: center.y + sin(a) * radius * 2)
                    )
                }
            }
            func moon(_ cx: Double, _ cy: Double, _ r: Double) {
                let radius = min(w, h) * r
                let center = CGPoint(x: w * cx, y: h * cy)
                var path = Path()
                path.addArc(center: center, radius: radius, startAngle: .degrees(300), endAngle: .degrees(580), clockwise: false)
                let inner = CGPoint(x: center.x + radius * 0.45, y: center.y - radius * 0.3)
                path.addArc(center: inner, radius: radius * 0.85, startAngle: .degrees(200), endAngle: .degrees(0), clockwise: true)
                stroke(path)
            }
            func cloud(_ cx: Double, _ cy: Double, _ width: Double) {
                let cw = w * width
                let ch = cw * 0.5
                let left = w * cx - cw / 2
                let top = h * cy - ch / 2
                var path = Path()
                path.move(to: CGPoint(x: left + ch * 0.5, y: top + ch))
                path.addArc(center: CGPoint(x: left + ch * 0.35, y: top + ch * 0.65), radius: ch * 0.35, startAngle: .degrees(90), endAngle: .degrees(270), clockwise: false)
                path.addArc(center: CGPoint(x: left + cw * 0.425, y: top - ch * 0.1 + cw * 0.225), radius: cw * 0.225, startAngle: .degrees(200), endAngle: .degrees(350), clockwise: false)
                path.addArc(center: CGPoint(x: left + cw * 0.58 + ch * 0.375, y: top + ch * 0.25 + ch * 0.375), radius: ch * 0.375, startAngle: .degrees(280), endAngle: .degrees(450), clockwise: false)
                path.closeSubpath()
                stroke(path)
            }
            let night = !isDay && (sky == .clear || sky == .partlyCloudy)
            switch sky {
            case .clear:
                if night { moon(0.5, 0.5, 0.3) } else { sun(0.5, 0.5, 0.18) }
            case .partlyCloudy:
                if night { moon(0.36, 0.34, 0.2) } else { sun(0.36, 0.34, 0.12) }
                cloud(0.58, 0.66, 0.62)
            case .cloudy:
                cloud(0.5, 0.56, 0.86)
            case .fog:
                for i in 0 ..< 3 {
                    let y = h * (0.34 + Double(i) * 0.16)
                    line(CGPoint(x: w * 0.18, y: y), CGPoint(x: w * 0.82, y: y))
                }
            default:
                cloud(0.5, 0.42, 0.8)
                let y0 = h * 0.7
                let y1 = h * 0.86
                switch sky {
                case .drizzle:
                    for x in [0.34, 0.5, 0.66] {
                        let r = width * 0.7
                        context.fill(Path(ellipseIn: CGRect(x: w * x - r, y: (y0 + y1) / 2 - r, width: r * 2, height: r * 2)), with: .color(color))
                    }
                case .snow:
                    for x in [0.34, 0.5, 0.66] {
                        let center = CGPoint(x: w * x, y: (y0 + y1) / 2)
                        let r = min(w, h) * 0.06
                        for i in 0 ..< 3 {
                            let a = Double(i) * .pi / 3
                            line(CGPoint(x: center.x - cos(a) * r, y: center.y - sin(a) * r), CGPoint(x: center.x + cos(a) * r, y: center.y + sin(a) * r))
                        }
                    }
                case .thunderstorm:
                    stroke(Path {
                        $0.move(to: CGPoint(x: w * 0.54, y: y0 - h * 0.04))
                        $0.addLine(to: CGPoint(x: w * 0.44, y: (y0 + y1) / 2))
                        $0.addLine(to: CGPoint(x: w * 0.56, y: (y0 + y1) / 2))
                        $0.addLine(to: CGPoint(x: w * 0.46, y: y1 + h * 0.04))
                    })
                default:
                    let slant = sky == .shower ? w * 0.06 : 0
                    for x in [0.34, 0.5, 0.66] {
                        line(CGPoint(x: w * x + slant, y: y0), CGPoint(x: w * x - slant, y: y1))
                    }
                }
            }
        }
    }
}

/// 날짜 옆 연결 아이콘
struct ViaIcon: View {
    let via: Via
    let color: Color

    var body: some View {
        Canvas { context, size in
            let w = size.width
            let h = size.height
            let style = StrokeStyle(lineWidth: min(w, h) * 0.09, lineCap: .round, lineJoin: .round)
            func stroke(_ points: [(Double, Double)]) {
                context.stroke(
                    Path { path in
                        path.addLines(points.map { CGPoint(x: w * $0.0, y: h * $0.1) })
                    },
                    with: .color(color),
                    style: style
                )
            }
            switch via {
            case .usb:
                stroke([(0.5, 0.12), (0.5, 0.82)])
                let r = w * 0.08
                context.fill(Path(ellipseIn: CGRect(x: w * 0.5 - r, y: h * 0.86 - r, width: r * 2, height: r * 2)), with: .color(color))
                stroke([(0.5, 0.62), (0.28, 0.48), (0.28, 0.34)])
                stroke([(0.5, 0.52), (0.72, 0.4), (0.72, 0.28)])
                stroke([(0.4, 0.2), (0.5, 0.06), (0.6, 0.2)])
            case .bluetooth:
                stroke([(0.28, 0.3), (0.7, 0.68), (0.5, 0.86), (0.5, 0.14), (0.7, 0.32), (0.28, 0.7)])
            }
        }
        .accessibilityLabel(via == .usb ? "USB" : "블루투스")
    }
}
