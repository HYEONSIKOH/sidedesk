import SwiftUI

/// 짝 맺기 숫자 확인, 붙지 못한 이유를 보이는 화면(안드로이드 `HomeScreen`)
struct HomeView: View {
    let model: HomeModel
    @Environment(\.palette) private var palette

    var body: some View {
        ZStack {
            palette.ground.ignoresSafeArea()
            if let code = model.pairingCode {
                pairing(code)
            } else {
                VStack(spacing: 0) {
                    Text(model.problem ?? model.computerName ?? "컴퓨터를 기다리는 중")
                        .font(.system(size: 28))
                        .foregroundStyle(palette.ink)
                        .multilineTextAlignment(.center)
                        .dim(model.dimmed)
                    if model.dimmed {
                        Text("연결 끊김")
                            .font(.system(size: 14))
                            .foregroundStyle(palette.ink)
                            .padding(.top, 8)
                    }
                    if let notice = model.notice {
                        Text(notice)
                            .font(.system(size: 12))
                            .foregroundStyle(palette.ink)
                            .multilineTextAlignment(.center)
                            .opacity(0.6)
                            .padding(.top, 16)
                    }
                }
                .padding(24)
            }
        }
    }

    private func pairing(_ code: String) -> some View {
        let shown = stride(from: 0, to: code.count, by: 3).map { start -> String in
            let from = code.index(code.startIndex, offsetBy: start)
            let to = code.index(from, offsetBy: min(3, code.count - start))
            return String(code[from ..< to])
        }.joined(separator: " ")
        return VStack(spacing: 16) {
            Text("컴퓨터에 표시된 번호와 같으면 등록을 누르세요.")
                .font(.system(size: 16, weight: .medium))
                .foregroundStyle(palette.ink)
            Text(shown)
                .font(.system(size: 45))
                .monospacedDigit()
                .foregroundStyle(palette.ink)
            HStack(spacing: 12) {
                Button("취소") { model.answer(false) }
                    .buttonStyle(.bordered)
                Button("등록") { model.answer(true) }
                    .buttonStyle(.borderedProminent)
            }
            .tint(palette.accent)
        }
        .padding(24)
    }
}
