import SwiftUI

/// 확인 창에 띄울 내용. 앱의 모든 확인 창은 이 모양 하나를 쓴다(안드로이드 `ConfirmModal` 과 같음).
struct Confirm: Equatable {
    var title: String
    /// 회색 설명, 두 줄까지
    var description: String
    var confirmLabel: String
    var cancelLabel = "취소"
    /// 되돌릴 수 없는 일이면 빨갛게
    var danger = false
    /// 위 동그라미에 넣을 글자 하나
    var glyph = "!"
}

private let appear = Animation.easeOut(duration: 0.18)

extension View {
    /// 확인 창이 떠 있을 때 뒤 화면을 흐리게 한다.
    func behindModal(_ open: Bool) -> some View {
        blur(radius: open ? 16 : 0).animation(appear, value: open)
    }
}

/// 화면 맨 위에 겹쳐 그린다. 판 밖을 누르면 onDismiss, 취소 버튼은 onCancel 이다.
struct ConfirmModal: View {
    let confirm: Confirm?
    let onConfirm: () -> Void
    let onDismiss: () -> Void
    var onCancel: (() -> Void)?

    @Environment(\.palette) private var palette

    var body: some View {
        ZStack {
            if let confirm {
                Color.black.opacity(0.6)
                    .ignoresSafeArea()
                    .contentShape(Rectangle())
                    .onTapGesture(perform: onDismiss)
                    .transition(.opacity)
                panel(confirm)
                    .transition(.scale(scale: 0.92).combined(with: .opacity))
            }
        }
        .animation(appear, value: confirm)
    }

    private func panel(_ confirm: Confirm) -> some View {
        let tint = confirm.danger ? palette.danger : palette.accent
        return VStack(spacing: 0) {
            Text(confirm.glyph)
                .font(.system(size: 22, weight: .semibold))
                .foregroundStyle(tint)
                .frame(width: 56, height: 56)
                .background(Circle().fill(tint.opacity(0.18)))
            Text(confirm.title)
                .font(.system(size: 20, weight: .bold))
                .foregroundStyle(palette.ink)
                .multilineTextAlignment(.center)
                .padding(.top, 16)
            Text(confirm.description)
                .font(.system(size: 14))
                .lineSpacing(6)
                .foregroundStyle(palette.muted)
                .lineLimit(2)
                .multilineTextAlignment(.center)
                .padding(.top, 8)
            HStack(spacing: 10) {
                pill(confirm.cancelLabel, palette.card, palette.ink, onCancel ?? onDismiss)
                pill(confirm.confirmLabel, tint, confirm.danger ? .white : palette.ground, onConfirm)
            }
            .padding(.top, 24)
        }
        .padding(24)
        .frame(maxWidth: 400)
        .background(RoundedRectangle(cornerRadius: 28).fill(palette.raised))
        .overlay(RoundedRectangle(cornerRadius: 28).stroke(Color.white.opacity(0.06), lineWidth: 1))
        // 판 안을 누르면 닫히지 않게 막는다.
        .contentShape(RoundedRectangle(cornerRadius: 28))
        .onTapGesture {}
        .padding(24)
    }

    private func pill(_ label: String, _ background: Color, _ ink: Color, _ action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Text(label)
                .font(.system(size: 16, weight: .semibold))
                .foregroundStyle(ink)
                .frame(maxWidth: .infinity)
                .frame(height: 48)
                .background(Capsule().fill(background))
        }
        .buttonStyle(.plain)
    }
}
