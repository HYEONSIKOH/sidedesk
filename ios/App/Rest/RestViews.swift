import SwiftUI

/// 쉬는 화면. 검은 판 위에 고른 시계 모양만 어둡게 그린다.
/// 모든 누름을 먼저 받아 깨우기만 하고, 아래로 보내지 않는다(아래 화면은 쉬는 동안 트리에서 빠져 있다).
struct RestScreen: View {
    let rest: RestModel
    let clock: ClockModel

    var body: some View {
        ZStack(alignment: .bottom) {
            Color.black.ignoresSafeArea()
            // 시각은 쉬는 판 안에서만, 분이 바뀔 때만 읽는다. 옮김 자리도 분 단위다.
            TimelineView(MinuteSchedule()) { context in
                let shift = shiftAt(minute: Int64(context.date.timeIntervalSince1970 / 60))
                HomeBig(clock: clock, now: context.date, resting: true) {}
                    .padding(24)
                    .offset(x: shift.x, y: shift.y)
            }
            if let alert = rest.alert {
                Text("\(alert.title) / \(alertText(alert.kind))")
                    .font(.system(size: 14))
                    .foregroundStyle(Colors.rest.ink)
                    .lineLimit(1)
                    .padding(.horizontal, 24)
                    .padding(.bottom, 28)
            }
        }
        .environment(\.palette, Colors.rest)
        .contentShape(Rectangle())
        .onTapGesture(perform: rest.touched)
        .gesture(DragGesture(minimumDistance: 0).onChanged { _ in rest.touched() })
        .transition(.asymmetric(insertion: .opacity.animation(.easeIn(duration: 0.6)), removal: .opacity.animation(.easeOut(duration: 0.2))))
        .statusBarHidden()
        .persistentSystemOverlays(.hidden)
    }
}

/// 홈 고르기 판 안의 쉬기 설정. 낮은 화면에서는 판 안에서 세로로 민다.
struct RestSettings: View {
    let rest: RestModel
    @Environment(\.palette) private var palette

    var body: some View {
        ScrollView(showsIndicators: false) {
            VStack(alignment: .leading, spacing: 6) {
                line("켜 두기") { ForEach(KeepOn.allCases, id: \.self) { value in chip(value.label, value == rest.keepOn) { rest.setKeepOn(value) } } }
                line("쉬기") { ForEach(RestAfter.allCases, id: \.self) { value in chip(value.label, value == rest.restAfter) { rest.setRestAfter(value) } } }
                line("알림에 깨우기") {
                    chip("켬", rest.wakeOnAlert) { rest.setWakeOnAlert(true) }
                    chip("끔", !rest.wakeOnAlert) { rest.setWakeOnAlert(false) }
                }
                line("진동") { ForEach(Vibration.allCases, id: \.self) { value in chip(value.label, value == rest.vibration) { rest.setVibration(value) } } }
                if !PhoneBuzzer.supported {
                    Text("이 기기는 진동을 낼 수 없습니다").font(.system(size: 11)).foregroundStyle(palette.muted).padding(.leading, 4)
                }
            }
        }
        .frame(maxHeight: 180)
    }

    private func line<Chips: View>(_ label: String, @ViewBuilder chips: () -> Chips) -> some View {
        HStack(spacing: 0) {
            Text(label).font(.system(size: 12)).foregroundStyle(palette.muted).lineLimit(1).frame(width: 84, alignment: .leading)
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 6) { chips() }
            }
        }
    }

    private func chip(_ label: String, _ on: Bool, _ action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Text(label)
                .font(.system(size: 13, weight: on ? .semibold : .regular))
                .foregroundStyle(on ? palette.panel : palette.ink)
                .lineLimit(1)
                .padding(.horizontal, 14)
                .padding(.vertical, 7)
                .background(Capsule().fill(on ? palette.accent : palette.card))
        }
        .buttonStyle(.plain)
    }
}
