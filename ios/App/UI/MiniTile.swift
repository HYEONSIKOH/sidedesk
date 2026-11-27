import SwiftUI

/// 작은 칸의 모양. 글자 크기는 칸 크기가 아니라 이 셋으로만 바뀐다.
struct MiniSize {
    /// 폭이 높이의 1.6배 이상: 한 줄로 놓는다
    let wide: Bool
    /// 폭 130 이하
    let narrow: Bool
    /// 높이 92 이하이고 한 줄이 아님
    let low: Bool

    init(width: CGFloat, height: CGFloat) {
        wide = width >= height * 1.6
        narrow = width <= 130
        low = height <= 92 && !wide
    }

    /// 큰 숫자
    var num: CGFloat { narrow ? 27 : 30 }
    /// 이름
    var name: CGFloat { narrow ? 15 : 16 }
    static let label: CGFloat = 14
    static let unit: CGFloat = 15
    static let mark: CGFloat = 22
}

/// 작은 칸 틀. 첫째는 위 왼쪽, 둘째는 아래 왼쪽(위아래로 벌림). 한 줄이면 양 끝으로 벌린다.
struct MiniTile<First: View, Second: View, Behind: View>: View {
    var background: Color?
    @ViewBuilder var behind: () -> Behind
    @ViewBuilder var first: (MiniSize) -> First
    @ViewBuilder var second: (MiniSize) -> Second
    @Environment(\.palette) private var palette

    var body: some View {
        GeometryReader { proxy in
            let size = MiniSize(width: proxy.size.width, height: proxy.size.height)
            let (h, v): (CGFloat, CGFloat) = size.low ? (11, 9) : size.narrow ? (12, 11) : (14, 12)
            ZStack {
                behind()
                Group {
                    if size.wide {
                        HStack(spacing: 10) {
                            first(size)
                            Spacer(minLength: 0)
                            second(size)
                        }
                    } else {
                        VStack(alignment: .leading, spacing: 0) {
                            first(size)
                            Spacer(minLength: 0)
                            second(size)
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
                .padding(.horizontal, h)
                .padding(.vertical, v)
            }
            .frame(width: proxy.size.width, height: proxy.size.height)
        }
        .background(background ?? palette.side)
        .clipShape(RoundedRectangle(cornerRadius: 20))
    }
}

extension MiniTile where Behind == EmptyView {
    init(background: Color? = nil, @ViewBuilder first: @escaping (MiniSize) -> First, @ViewBuilder second: @escaping (MiniSize) -> Second) {
        self.init(background: background, behind: { EmptyView() }, first: first, second: second)
    }
}

/// 작은 칸의 큰 숫자. 단위(% 등)는 15 로 붙인다.
struct MiniNum: View {
    let text: String
    var unit: String?
    let size: MiniSize
    @Environment(\.palette) private var palette

    var body: some View {
        (Text(text).font(.system(size: size.num, weight: .medium))
            + Text(unit ?? "").font(.system(size: MiniSize.unit, weight: .medium)))
            .tracking(-0.9)
            .foregroundStyle(palette.ink)
            .lineLimit(1)
            .fixedSize()
    }
}

/// 작은 칸의 흐린 라벨(14)
struct MiniLabel: View {
    let text: String
    @Environment(\.palette) private var palette

    var body: some View {
        Text(text).font(.system(size: MiniSize.label, weight: .medium)).foregroundStyle(palette.muted).lineLimit(1)
    }
}

/// 작은 칸의 이름(16, 좁으면 15). 낮거나 한 줄이면 한 줄, 아니면 두 줄까지
struct MiniName: View {
    let text: String
    let size: MiniSize
    @Environment(\.palette) private var palette

    var body: some View {
        Text(text)
            .font(.system(size: size.name, weight: .medium))
            .foregroundStyle(palette.ink)
            .lineLimit(size.low || size.wide ? 1 : 2)
            .multilineTextAlignment(.leading)
    }
}
