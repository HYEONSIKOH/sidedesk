import SwiftUI

/// 앱 색 한 벌. 안드로이드 `Palette` 와 같은 값(docs/images 의 screens, hero 그림에서 뽑음).
struct Colors: Sendable {
    let ground: Color
    let panel: Color
    let card: Color
    let raised: Color
    let side: Color
    let liquid: Color
    let accent: Color
    let ink: Color
    let muted: Color
    let danger: Color
    /// 시계 숫자 위아래 그라데이션
    let digitTop: Color
    let digitBottom: Color
    let dark: Bool

    static let dark = Colors(
        ground: Color(hex: 0x15181C),
        panel: Color(hex: 0x22272E),
        card: Color(hex: 0x2C323A),
        raised: Color(hex: 0x353C45),
        side: Color(hex: 0x1D2127),
        liquid: Color(hex: 0x33475A),
        accent: Color(hex: 0x8FAECB),
        ink: Color(hex: 0xDCE1E7),
        muted: Color(hex: 0x8B939E),
        danger: Color(hex: 0xE5484D),
        digitTop: Color(hex: 0xD3DEE9),
        digitBottom: Color(hex: 0x7D93AB),
        dark: true
    )

    static let light = Colors(
        ground: Color(hex: 0xE6E9ED),
        panel: Color(hex: 0xF6F7F9),
        card: Color(hex: 0xE5E9EE),
        raised: Color(hex: 0xFFFFFF),
        side: Color(hex: 0xF1F3F6),
        liquid: Color(hex: 0xCBD8E4),
        accent: Color(hex: 0x4F7090),
        ink: Color(hex: 0x2C333C),
        muted: Color(hex: 0x6E7783),
        danger: Color(hex: 0xD93D42),
        digitTop: Color(hex: 0x7F97B0),
        digitBottom: Color(hex: 0xAABCCE),
        dark: false
    )

    static func of(_ theme: Theme) -> Colors {
        theme == .light ? .light : .dark
    }
}

extension Color {
    init(hex: UInt32) {
        self.init(
            red: Double((hex >> 16) & 0xFF) / 255,
            green: Double((hex >> 8) & 0xFF) / 255,
            blue: Double(hex & 0xFF) / 255
        )
    }
}

private struct PaletteKey: EnvironmentKey {
    static let defaultValue = Colors.dark
}

extension EnvironmentValues {
    /// 지금 테마의 색
    var palette: Colors {
        get { self[PaletteKey.self] }
        set { self[PaletteKey.self] = newValue }
    }
}

/// 끊기면 컴퓨터에서 온 값만 흐리게 한다. 시계는 흐리지 않는다.
let dimAlpha = 0.4

extension View {
    func dim(_ dimmed: Bool) -> some View {
        opacity(dimmed ? dimAlpha : 1)
    }
}
