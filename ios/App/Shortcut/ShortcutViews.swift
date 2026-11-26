import SwiftUI

private let gap = 12.0

/// 단축키 큰 화면. 누르면 번호만 보내고, 실패할 때만 판 아래에 한 줄 보인다.
struct ShortcutBig: View {
    let shortcuts: ShortcutModel
    @Environment(\.palette) private var palette

    var body: some View {
        GeometryReader { proxy in
            ZStack(alignment: .bottom) {
                Group {
                    if let items = shortcuts.items, !items.isEmpty {
                        Keys(items: items, grid: shortcutGrid(width: proxy.size.width, height: proxy.size.height, count: items.count), press: shortcuts.press)
                    } else if shortcuts.items != nil {
                        Text("컴퓨터의 SideDesk 설정에서 단축키를 더하세요")
                            .font(.system(size: 15))
                            .foregroundStyle(palette.muted)
                            .multilineTextAlignment(.center)
                            .padding(16)
                            .frame(maxWidth: .infinity, maxHeight: .infinity)
                    }
                }
                .dim(shortcuts.dimmed)
                if let notice = shortcuts.notice {
                    NoticePill(text: notice)
                }
            }
        }
    }
}

/// 판 아래에 잠깐 뜨는 한 줄(실패 알림)
struct NoticePill: View {
    let text: String
    @Environment(\.palette) private var palette

    var body: some View {
        Text(text)
            .font(.system(size: 14))
            .foregroundStyle(palette.ground)
            .multilineTextAlignment(.center)
            .padding(.horizontal, 18)
            .padding(.vertical, 10)
            .background(Capsule().fill(palette.ink))
            .padding(8)
    }
}

private struct Keys: View {
    let items: [Shortcut]
    let grid: ShortcutGrid
    let press: (UInt32) -> Void

    var body: some View {
        let rows = stride(from: 0, to: items.count, by: grid.columns).map { Array(items[$0 ..< min($0 + grid.columns, items.count)]) }
        let content = VStack(spacing: gap) {
            ForEach(rows.indices, id: \.self) { index in
                HStack(spacing: gap) {
                    ForEach(rows[index]) { item in
                        Key(item: item, size: grid.cell) { press(item.id) }
                    }
                    ForEach(0 ..< grid.columns - rows[index].count, id: \.self) { _ in
                        Color.clear.frame(width: grid.cell, height: grid.cell)
                    }
                }
            }
        }
        if grid.scrolls {
            ScrollView(showsIndicators: false) { content }
                .mask(VStack(spacing: 0) {
                    Color.black
                    LinearGradient(colors: [.black, .clear], startPoint: .top, endPoint: .bottom).frame(height: 36)
                })
        } else {
            content.frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }
}

/// 칸 하나(시안 .ky). 보통 버튼은 색 바탕 둥근 네모에 흰 그림, 시스템 버튼은 칸에 색 그림만,
/// 켜진 버튼은 칸 전체를 그 색으로 채운다. 누르면 0.9 로 줄었다 돌아온다.
private struct Key: View {
    let item: Shortcut
    let size: Double
    let press: () -> Void
    @Environment(\.palette) private var palette

    var body: some View {
        Button(action: press) {
            let color = shortcutColor(item.color)
            let on = item.toggle == .on
            let named = !item.name.trimmingCharacters(in: .whitespaces).isEmpty
            ZStack(alignment: .bottom) {
                Group {
                    if on {
                        Glyph(icon: item.icon, fallback: item.name, color: onColor(item.color)).frame(width: size * 0.5, height: size * 0.5)
                    } else if item.system {
                        Glyph(icon: item.icon, fallback: item.name, color: color).frame(width: size * 0.5, height: size * 0.5)
                    } else {
                        let box = size * (named ? 0.6 : 0.74)
                        Glyph(icon: item.icon, fallback: item.name, color: .white)
                            .frame(width: size * 0.42, height: size * 0.42)
                            .frame(width: box, height: box)
                            .background(RoundedRectangle(cornerRadius: size * 0.16).fill(color))
                    }
                }
                .padding(.bottom, named ? size * 0.12 : 0)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                if named {
                    Text(item.name)
                        .font(.system(size: min(12, size * 0.12)))
                        .foregroundStyle(on ? onColor(item.color).opacity(0.8) : palette.muted)
                        .lineLimit(1)
                        .padding(.horizontal, 6)
                        .padding(.vertical, size * 0.05)
                }
            }
            .frame(width: size, height: size)
            .background(RoundedRectangle(cornerRadius: size * 0.2).fill(on ? color : palette.card))
            .contentShape(RoundedRectangle(cornerRadius: size * 0.2))
        }
        .buttonStyle(Squeeze())
    }
}

/// 누르면 0.9 로 줄었다 돌아온다.
private struct Squeeze: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .scaleEffect(configuration.isPressed ? 0.9 : 1)
            .animation(.easeOut(duration: 0.12), value: configuration.isPressed)
    }
}

/// 작은 칸(시안 .mdeck). 앞 네 개의 그림을 작게 놓는다. 좁으면 2×2
struct ShortcutTile: View {
    let shortcuts: ShortcutModel
    @Environment(\.palette) private var palette

    var body: some View {
        GeometryReader { proxy in
            let first = Array((shortcuts.items ?? []).prefix(4))
            Group {
                if first.isEmpty {
                    Text("단축키").font(.system(size: 14)).foregroundStyle(palette.ink).lineLimit(1)
                } else {
                    let row = proxy.size.width >= 4 * 26 + 3 * 6
                    let icon = min(28, row ? (proxy.size.width - 18) / 4 : min((proxy.size.width - 6) / 2, (proxy.size.height - 6) / 2))
                    Group {
                        if row {
                            HStack(spacing: 6) { ForEach(first) { Mini(item: $0, size: icon) } }
                        } else {
                            VStack(spacing: 6) {
                                ForEach(stride(from: 0, to: first.count, by: 2).map { $0 }, id: \.self) { start in
                                    HStack(spacing: 6) { ForEach(first[start ..< min(start + 2, first.count)]) { Mini(item: $0, size: icon) } }
                                }
                            }
                        }
                    }
                    .dim(shortcuts.dimmed)
                }
            }
            .frame(width: proxy.size.width, height: proxy.size.height)
        }
        .padding(8)
        .background(RoundedRectangle(cornerRadius: 20).fill(palette.side))
    }
}

private struct Mini: View {
    let item: Shortcut
    let size: Double

    var body: some View {
        let color = shortcutColor(item.color)
        if item.system, item.toggle != .on {
            Glyph(icon: item.icon, fallback: item.name, color: color).frame(width: size, height: size)
        } else {
            Glyph(icon: item.icon, fallback: item.name, color: item.toggle == .on ? onColor(item.color) : .white)
                .frame(width: size * 0.62, height: size * 0.62)
                .frame(width: size, height: size)
                .background(RoundedRectangle(cornerRadius: size * 0.22).fill(color))
        }
    }
}
