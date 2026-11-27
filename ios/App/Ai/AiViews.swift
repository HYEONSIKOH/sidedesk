import SwiftUI

private let gap = 8.0
/// 세션 카드가 이보다 작아지면 줄이지 않고 목록을 민다.
private let minUnit = 56.0

private func toolText(_ tool: AiTool) -> String? {
    switch tool {
    case .claudeCode: "Claude"
    case .codex: "Codex"
    case .unknown: nil
    }
}

/// AI 큰 화면. 위는 세션 카드, 아래는 GitHub/GitLab 숫자
struct AiBig: View {
    let ai: AiModel
    @Environment(\.palette) private var palette

    var body: some View {
        VStack(spacing: 10) {
            GeometryReader { proxy in
                let sessions = ai.sessions
                Group {
                    if sessions.isEmpty {
                        if ai.busy != nil {
                            Text("실행 중인 AI 세션이 없습니다")
                                .font(.system(size: 15))
                                .foregroundStyle(palette.muted)
                                .multilineTextAlignment(.center)
                                .frame(maxWidth: .infinity, maxHeight: .infinity)
                        }
                    } else {
                        // 지난 시간은 이 판 안에서만, 분이 바뀔 때만 읽는다.
                        TimelineView(MinuteSchedule()) { context in
                            Sessions(sessions: sessions, highlight: ai.alert?.sessionId, now: Int64(context.date.timeIntervalSince1970 * 1000), height: proxy.size.height)
                        }
                    }
                }
                .dim(ai.dimmed)
            }
            ForgeBar(ai: ai)
        }
    }
}

/// 작업 중과 답변 대기는 판 너비 전체(높이 1.3u), 나머지는 반 너비 둘씩(높이 u).
/// u 는 76pt 를 넘지 않고, 다 넣으려면 56pt 아래가 될 때는 56pt 로 두고 민다.
/// 세션 칸 높이의 상한과 작업 중 칸의 배수
let maxUnit: Double = 76
let activeRatio: Double = 1.3

func sessionUnit(height: Double, active: Int, rest: Int) -> Double {
    let pairs = Double((rest + 1) / 2)
    let rows = Double(active) * activeRatio + pairs
    let count = Double(active) + pairs
    guard rows > 0 else { return maxUnit }
    return max(minUnit, min(maxUnit, (height - (count - 1) * gap) / rows))
}

private struct Sessions: View {
    let sessions: [AiSession]
    let highlight: String?
    let now: Int64
    let height: Double
    @Environment(\.palette) private var palette

    var body: some View {
        let active = sessions.filter(\.active)
        let rest = sessions.filter { !$0.active }
        let unit = sessionUnit(height: height, active: active.count, rest: rest.count)
        let pairs = stride(from: 0, to: rest.count, by: 2).map { Array(rest[$0 ..< min($0 + 2, rest.count)]) }
        ScrollView(showsIndicators: false) {
            VStack(spacing: gap) {
                ForEach(active) { session in
                    SessionCard(session: session, now: now, highlight: session.id == highlight, big: true)
                        .frame(height: unit * activeRatio)
                }
                ForEach(pairs.indices, id: \.self) { index in
                    HStack(spacing: gap) {
                        ForEach(pairs[index]) { session in
                            SessionCard(session: session, now: now, highlight: session.id == highlight, big: false)
                        }
                        if pairs[index].count == 1 { Color.clear }
                    }
                    .frame(height: unit)
                }
            }
        }
        // 아래로 더 있으면 끝을 판 색으로 흐리게 사라지게 해, 밀어 볼 수 있다는 것을 알린다.
        .mask(
            VStack(spacing: 0) {
                Color.black
                LinearGradient(colors: [.black, .black.opacity(contentHeight(unit, active.count, pairs.count) > height ? 0 : 1)], startPoint: .top, endPoint: .bottom)
                    .frame(height: 36)
            }
        )
    }

    private func contentHeight(_ unit: Double, _ active: Int, _ pairs: Int) -> Double {
        Double(active) * unit * activeRatio + Double(pairs) * unit + Double(max(0, active + pairs - 1)) * gap
    }
}

private struct SessionCard: View {
    let session: AiSession
    let now: Int64
    let highlight: Bool
    let big: Bool
    @Environment(\.palette) private var palette

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: big ? 20 : 16)
        let (background, edge): (Color, Color) = switch session.state {
        case .working: (palette.hot, palette.accent.opacity(0.35))
        case .waiting: (palette.card, palette.waiting.opacity(0.6))
        default: (palette.card, .clear)
        }
        VStack(alignment: .leading, spacing: 2) {
            HStack(spacing: 6) {
                Text(session.title)
                    .font(.system(size: big ? 16 : 14, weight: .semibold))
                    .foregroundStyle(palette.ink)
                    .lineLimit(1)
                    .frame(maxWidth: .infinity, alignment: .leading)
                Text(elapsedText(now: now, since: session.since))
                    .font(.system(size: 12))
                    .foregroundStyle(palette.muted)
                    .lineLimit(1)
            }
            HStack(spacing: 6) {
                if let tool = toolText(session.tool) {
                    Text(tool)
                        .font(.system(size: 10))
                        .foregroundStyle(palette.muted)
                        .padding(.horizontal, 5)
                        .padding(.vertical, 1)
                        .background(RoundedRectangle(cornerRadius: 6).fill(palette.side))
                }
                Text([stateText(session.state), session.project].filter { !$0.isEmpty }.joined(separator: ", "))
                    .font(.system(size: big ? 13 : 12))
                    .foregroundStyle(session.state == .waiting ? palette.waiting : palette.muted)
                    .lineLimit(1)
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, big ? 12 : 8)
        .opacity(session.open ? 1 : 0.6)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
        .background(
            ZStack {
                shape.fill(background)
                // 답변 대기는 카드 바탕에 주황을 옅게 겹친다.
                if session.state == .waiting { shape.fill(palette.waiting.opacity(0.16)) }
            }
        )
        .overlay(shape.stroke(highlight ? palette.accent : edge, lineWidth: highlight ? 2 : 1))
    }
}

/// 아래 줄: GitHub/GitLab 고르기와 검토, 내 요청, 알림 수
private struct ForgeBar: View {
    let ai: AiModel
    @Environment(\.palette) private var palette

    var body: some View {
        HStack(spacing: 0) {
            HStack(spacing: 0) {
                ForEach(ForgeTab.allCases, id: \.self) { tab in
                    let on = tab == ai.tab
                    Text(tab.rawValue)
                        .font(.system(size: 13, weight: on ? .semibold : .regular))
                        .foregroundStyle(on ? palette.panel : palette.ink)
                        .lineLimit(1)
                        .padding(.horizontal, 12)
                        .padding(.vertical, 8)
                        .background(RoundedRectangle(cornerRadius: 11).fill(on ? palette.accent : .clear))
                        .contentShape(Rectangle())
                        .onTapGesture { ai.select(tab) }
                }
            }
            .padding(3)
            .background(RoundedRectangle(cornerRadius: 14).fill(palette.side))
            Group {
                if let counts = ai.counts, counts.state == .ok {
                    HStack {
                        Spacer()
                        count("\(counts.reviews)", "검토")
                        Spacer()
                        count("\(counts.mine)", "내 요청")
                        Spacer()
                        count(noticeText(counts.notices), "알림")
                        Spacer()
                    }
                } else {
                    Text(forgeProblem(ai.counts?.state, tool: ai.tab == .github ? "gh" : "glab"))
                    .font(.system(size: 13))
                    .foregroundStyle(palette.muted)
                    .lineLimit(2)
                    .multilineTextAlignment(.center)
                    .padding(.horizontal, 8)
                }
            }
            .frame(maxWidth: .infinity)
            .dim(ai.dimmed)
        }
        .padding(.horizontal, 10)
        .frame(height: 64)
        .background(RoundedRectangle(cornerRadius: 18).fill(palette.card))
    }

    private func count(_ value: String, _ label: String) -> some View {
        VStack(spacing: 0) {
            Text(value).font(.system(size: 20, weight: .medium)).foregroundStyle(palette.ink)
            Text(label).font(.system(size: 11)).foregroundStyle(palette.muted)
        }
        .lineLimit(1)
    }
}

/// GitHub/GitLab 숫자 대신 보일 문장
func forgeProblem(_ state: ForgeState?, tool: String) -> String {
    switch state {
    case .missing?: "\(tool) 가 설치되어 있지 않습니다"
    case .signedOut?: "\(tool) auth login 으로 로그인하세요"
    case .failed?: "불러오지 못했습니다"
    default: "-"
    }
}

/// AI 작은 칸. "AI n 작업 중", 일이 있으면 hot 바탕, 답변 대기가 있으면 주황 점.
/// 알림이 오면 살짝 커졌다 돌아오고 4초 동안 제목과 바뀐 상태를 보인다.
/// 알림 카드. 반전 색 판이 왼쪽 위에서 동그랗게 퍼지며 칸을 덮는다.
private struct AlertCard: View {
    let alert: AiAlert
    @Environment(\.palette) private var palette
    @State private var reveal = 0.0

    var body: some View {
        GeometryReader { proxy in
            let far = hypot(proxy.size.width - 24, proxy.size.height - 24) + 24
            VStack(alignment: .leading, spacing: 2) {
                Text(alert.title).font(.system(size: 17, weight: .bold))
                Text(alertText(alert.kind)).font(.system(size: 13))
            }
            .foregroundStyle(palette.ground)
            .lineLimit(1)
            .padding(.horizontal, 14)
            .frame(width: proxy.size.width, height: proxy.size.height, alignment: .leading)
            .background(palette.ink)
            .clipShape(Circle().size(width: far * 2 * reveal, height: far * 2 * reveal).offset(x: 24 - far * reveal, y: 24 - far * reveal))
        }
        .onAppear { withAnimation(.timingCurve(0.2, 0.9, 0.2, 1, duration: 0.45)) { reveal = 1 } }
    }
}

/// 화면 위쪽 가운데로 내려오는 알림 띠. 알림이 있는 동안(4초) 보인다. 늦게 온 알림은 띄우지 않는다.
struct AlertBanner: View {
    let ai: AiModel
    @Environment(\.palette) private var palette

    var body: some View {
        let alert = ai.alertLate ? nil : ai.alert
        VStack {
            if let alert {
                let waiting = alert.kind == .waiting
                HStack(spacing: 0) {
                    Text(waiting ? "!" : "✓")
                        .font(.system(size: 14, weight: .bold))
                        .frame(width: 26, height: 26)
                        .background(Circle().fill(palette.ground.opacity(0.25)))
                    Text(alertText(alert.kind)).font(.system(size: 14, weight: .bold)).padding(.leading, 10)
                    Text(alert.title).font(.system(size: 14)).padding(.leading, 8).frame(maxWidth: 280, alignment: .leading).fixedSize(horizontal: true, vertical: false)
                }
                .foregroundStyle(palette.ground)
                .lineLimit(1)
                .padding(.leading, 8)
                .padding(.trailing, 18)
                .padding(.vertical, 8)
                .background(Capsule().fill(waiting ? palette.waiting : palette.accent))
                .padding(.top, 10)
                .transition(.move(edge: .top).combined(with: .opacity))
            }
            Spacer()
        }
        .animation(.easeOut(duration: 0.3), value: alert?.id)
        .allowsHitTesting(false)
    }
}

struct AiTile: View {
    let ai: AiModel
    @Environment(\.palette) private var palette
    @State private var pulse = 1.0

    var body: some View {
        let busy = ai.busy ?? 0
        let alert = ai.alert
        GeometryReader { proxy in
            ZStack {
                if let alert {
                    AlertCard(alert: alert).id(alert.id)
                } else {
                    let count = ai.busy.map(String.init) ?? "-"
                    MiniTile(background: .clear) { _ in
                        Text("AI").font(.system(size: MiniSize.mark, weight: .medium)).tracking(-0.44).foregroundStyle(palette.ink)
                    } second: { size in
                        HStack(alignment: .lastTextBaseline, spacing: 6) {
                            MiniNum(text: count, size: size)
                            MiniLabel(text: "작업 중")
                        }
                    }
                    .dim(ai.dimmed)
                    if ai.waiting {
                        Circle().fill(palette.waiting).frame(width: 7, height: 7)
                            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topTrailing)
                            .padding(.top, 12)
                            .padding(.trailing, 14)
                    }
                }
            }
            .frame(width: proxy.size.width, height: proxy.size.height)
        }
        .background(RoundedRectangle(cornerRadius: 20).fill(busy > 0 || alert != nil ? palette.hot : palette.side))
        .clipShape(RoundedRectangle(cornerRadius: 20))
        .scaleEffect(pulse)
        .onChange(of: alert?.id) { _, id in
            guard id != nil else { return }
            withAnimation(.easeOut(duration: 0.14)) { pulse = 1.06 } completion: {
                withAnimation(.easeOut(duration: 0.22)) { pulse = 1 }
            }
        }
    }
}
