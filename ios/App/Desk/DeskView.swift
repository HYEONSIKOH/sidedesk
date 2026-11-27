import SwiftUI

/// 왼쪽(세로면 아래) 큰 화면과 작은 칸. 확인 창은 공통 `ConfirmModal` 하나.
struct DeskView: View {
    let desk: DeskModel
    let clock: ClockModel
    let ai: AiModel
    let shortcuts: ShortcutModel
    let music: MusicModel
    /// 고르기 판의 쉬기 설정
    var rest: RestModel?
    /// 고르기 판을 쉬기 설정부터 열지(그림 시험용)
    var restOpen = false
    /// 블루투스를 쓸 수 없는 이유. USB 는 된다.
    let notice: String?
    /// 끝내기 확인이 없을 때 띄울 안내 창(위치)
    var modal: Confirm?
    /// 안내 창 답. true 확인, false 취소 버튼, nil 바깥 누르기
    var onModal: (Bool?) -> Void = { _ in }

    @Environment(\.palette) private var palette

    var body: some View {
        let shown = desk.asking.map(Self.killConfirm) ?? modal
        ZStack {
            palette.ground.ignoresSafeArea()
            // 시각은 쓰는 칸(홈 큰 화면, 홈 작은 칸, AI 지난 시간)이 각자 분 단위로 읽는다(SD-14).
            content
                .behindModal(shown != nil)
            AlertBanner(ai: ai)
            ConfirmModal(
                confirm: shown,
                onConfirm: { desk.asking != nil ? desk.confirmKill() : onModal(true) },
                onDismiss: { desk.asking != nil ? desk.cancel() : onModal(nil) },
                onCancel: { desk.asking != nil ? desk.cancel() : onModal(false) }
            )
        }
    }

    static func killConfirm(_ process: ComputerProcess) -> Confirm {
        Confirm(
            title: "\(eul(process.name)) 끝낼까요?",
            description: String(format: "CPU %.1f%%, 메모리 %@\n저장하지 않은 내용은 사라질 수 있습니다", process.cpu, memoryText(process.memoryBytes)),
            confirmLabel: "끝내기",
            danger: true,
            glyph: process.name.first.map(String.init) ?? "!"
        )
    }

    private var content: some View {
        Zoomed { board }
    }

    private var board: some View {
        GeometryReader { proxy in
            let wide = proxy.size.width >= proxy.size.height
            Group {
                if wide {
                    // 화면 비율로 옆 줄을 정한다. 4:3 근처는 한 줄, 16:9 처럼 긴 화면은 두 줄.
                    let side = sideLayout(width: proxy.size.width - 24, height: proxy.size.height - 24)
                    HStack(spacing: 10) {
                        big
                        VStack(spacing: 10) {
                            ForEach(sideRows(desk.small, columns: side.columns), id: \.self) { row in
                                HStack(spacing: 10) {
                                    tiles(row).frame(maxWidth: .infinity, maxHeight: .infinity)
                                }
                            }
                        }
                        .frame(width: side.width)
                    }
                } else {
                    VStack(spacing: 12) {
                        // 작은 칸이 다섯이면 한 줄에 넣기에 좁아 두 줄(위 셋, 아래 둘)로 나눈다.
                        ForEach(tileRows(desk.small), id: \.self) { row in
                            HStack(spacing: 10) {
                                tiles(row).frame(maxWidth: .infinity).frame(height: 72)
                            }
                        }
                        big
                    }
                }
            }
            .padding(12)
        }
    }

    @ViewBuilder
    private func tiles(_ screens: [Screen]) -> some View {
        // 고르는 중에 작은 칸을 누르면 고르기를 닫고 그 화면으로 간다.
        ForEach(screens, id: \.self) { screen in
            Group {
                switch screen {
                case .home:
                    TimelineView(MinuteSchedule()) { context in HomeTile(clock: clock, now: context.date) }
                case .ai: AiTile(ai: ai)
                case .perf: PerfTile(cpu: desk.perf?.cpu, memory: desk.perf?.memory).dim(desk.dimmed)
                case .processes: TopProcessTile(top: desk.rows.first).dim(desk.dimmed)
                case .shortcuts: ShortcutTile(shortcuts: shortcuts)
                case .music: MusicTile(music: music)
                }
            }
            .contentShape(Rectangle())
            .onTapGesture {
                clock.done()
                desk.show(screen)
            }
        }
    }

    private var big: some View {
        let home = desk.big == .home
        let picking = home && clock.picking
        return ZStack {
            switch desk.big {
            case .home:
                TimelineView(MinuteSchedule()) { context in
                    HomeBig(clock: clock, now: context.date) {
                        // 길게 눌러 고르는 중이면 출처 링크를 열지 않는다.
                        if !clock.picking, let url = URL(string: "https://open-meteo.com/") {
                            UIApplication.shared.open(url)
                        }
                    }
                }
            case .ai:
                AiBig(ai: ai)
            case .perf:
                PerfScreen(desk: desk).dim(desk.dimmed)
            case .processes:
                ProcessScreen(desk: desk).dim(desk.dimmed)
            case .shortcuts:
                ShortcutBig(shortcuts: shortcuts)
            case .music:
                MusicBig(music: music)
            }
            if home, !picking, let notice {
                VStack {
                    Text(notice).font(.system(size: 12)).foregroundStyle(palette.muted).multilineTextAlignment(.center)
                    Spacer()
                }
            }
            if picking {
                VStack {
                    Spacer()
                    FacePicker(clock: clock, rest: rest, restOpen: restOpen)
                }
            }
            if let notice = desk.notice {
                VStack {
                    // 고르기 줄과 겹치지 않게 고르는 중에는 위에 띄운다.
                    if !picking { Spacer() }
                    Text(notice)
                        .font(.system(size: 14))
                        .foregroundStyle(palette.ground)
                        .padding(.horizontal, 18)
                        .padding(.vertical, 10)
                        .background(Capsule().fill(palette.ink))
                        .padding(.vertical, 8)
                    if picking { Spacer() }
                }
                .task(id: notice) {
                    try? await Task.sleep(for: .seconds(4))
                    if !Task.isCancelled { desk.dismissNotice() }
                }
            }
        }
        .padding(14)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(RoundedRectangle(cornerRadius: 28).fill(palette.panel))
        .clipShape(RoundedRectangle(cornerRadius: 28))
        .contentShape(RoundedRectangle(cornerRadius: 28))
        // 옆으로 60pt 넘게 밀면 다음 화면, 고르는 중이면 다음 모양.
        // 안쪽 목록의 세로 스크롤을 막지 않게 함께 받는다. 가로가 더 클 때만 민 것으로 본다.
        .simultaneousGesture(
            DragGesture(minimumDistance: 20).onEnded { drag in
                switch swipeStep(drag.translation) {
                case 1: picking ? clock.nextFace() : desk.next()
                case -1: picking ? clock.previousFace() : desk.previous()
                default: break
                }
            }
        )
        // 홈에서는 판 어디를 길게 눌러도 고르기. 손가락이 움직이면 밀기다.
        .simultaneousGesture(
            LongPressGesture(minimumDuration: 0.5, maximumDistance: 10).onEnded { _ in
                if desk.big == .home { clock.pick() }
            }
        )
    }
}

private struct PerfScreen: View {
    let desk: DeskModel
    @Environment(\.palette) private var palette

    var body: some View {
        let perf = desk.perf
        VStack(spacing: 10) {
            HStack(spacing: 10) {
                PerfGauge(label: "CPU", value: perf?.cpu)
                PerfGauge(label: "GPU", value: perf?.gpu)
                PerfGauge(label: "메모리", value: perf?.memory)
                PerfGauge(label: "디스크", value: perf?.disk)
            }
            HStack(spacing: 0) {
                Rate(arrow: "↓", bytes: perf?.receivePerSecond)
                NetGraph(received: desk.received, sent: desk.sent)
                    .padding(.horizontal, 14)
                    .padding(.vertical, 10)
                Rate(arrow: "↑", bytes: perf?.sendPerSecond)
            }
            .padding(.horizontal, 16)
            .frame(height: 56)
            .background(RoundedRectangle(cornerRadius: 18).fill(palette.card))
        }
    }
}

private struct PerfGauge: View {
    let label: String
    let value: Float?
    @Environment(\.palette) private var palette

    var body: some View {
        GeometryReader { proxy in
            // 좁은 칸에서도 숫자와 % 가 한 줄에 들어가게 칸 너비에 맞춘다.
            let inner = proxy.size.width - 28
            let size = min(38, inner / 1.6)
            let labelSize = min(14, inner / 3)
            ZStack(alignment: .topLeading) {
                Liquid(value: value)
                VStack(alignment: .leading, spacing: 0) {
                    Text(label).font(.system(size: labelSize)).foregroundStyle(palette.ink).lineLimit(1)
                    Percent(value: value, size: size)
                }
                .padding(14)
            }
        }
        .background(RoundedRectangle(cornerRadius: 22).fill(palette.card))
        .clipShape(RoundedRectangle(cornerRadius: 22))
    }
}

private struct Percent: View {
    let value: Float?
    let size: Double
    @Environment(\.palette) private var palette

    var body: some View {
        (Text(value.flatMap { $0.isFinite && abs($0) < 1e6 ? "\(Int($0))" : nil } ?? "-").font(.system(size: size, weight: .medium))
            + Text("%").font(.system(size: size * 0.45, weight: .medium)))
            .foregroundStyle(palette.ink)
            .lineLimit(1)
            .fixedSize()
    }
}

private struct Rate: View {
    let arrow: String
    let bytes: Int64?
    @Environment(\.palette) private var palette

    var body: some View {
        let (number, unit) = rateText(bytes)
        (Text("\(arrow)  ").font(.system(size: 14))
            + Text(number).font(.system(size: 20, weight: .medium))
            + Text(" \(unit)").font(.system(size: 12)))
            .foregroundStyle(palette.ink)
            .lineLimit(1)
            .fixedSize()
    }
}

private struct NetGraph: View {
    let received: [Int64]
    let sent: [Int64]
    @Environment(\.palette) private var palette

    var body: some View {
        Canvas { context, size in
            let top = Double(max(received.max() ?? 0, sent.max() ?? 0, 1))
            func line(_ values: [Int64], _ color: Color) {
                guard values.count >= 2 else { return }
                let step = size.width / Double(DeskModel.history - 1)
                let start = size.width - step * Double(values.count - 1)
                var path = Path()
                for (i, v) in values.enumerated() {
                    let point = CGPoint(x: start + step * Double(i), y: size.height * (1 - Double(v) / top))
                    if i == 0 { path.move(to: point) } else { path.addLine(to: point) }
                }
                context.stroke(path, with: .color(color), style: StrokeStyle(lineWidth: 1.5, lineCap: .round, lineJoin: .round))
            }
            line(sent, palette.muted)
            line(received, palette.accent)
        }
    }
}

private struct ProcessScreen: View {
    let desk: DeskModel
    @Environment(\.palette) private var palette

    var body: some View {
        let rows = desk.rows
        let top: Double = {
            let biggest = switch desk.sort {
            case .cpu: rows.map { Double($0.cpu) }.max()
            case .memory: rows.map { Double($0.memoryBytes) }.max()
            }
            return (biggest ?? 0) > 0 ? biggest! : 1
        }()
        VStack(spacing: 0) {
            HStack(spacing: 0) {
                // 고른 기준(CPU, 메모리)의 컴퓨터 전체 사용률
                Percent(value: desk.sort == .cpu ? desk.perf?.cpu : desk.perf?.memory, size: 34)
                Text(desk.sort == .cpu ? "CPU 사용률" : "메모리 사용률").font(.system(size: 12)).foregroundStyle(palette.ink).padding(.leading, 10)
                Spacer()
                sortButtons
            }
            .padding(.leading, 4)
            .padding(.bottom, 10)
            ScrollViewReader { reader in
                ScrollView {
                    // 순서가 바뀔 때 목록이 그 줄을 따라가지 않게 자리 번호로 둔다.
                    LazyVStack(spacing: 8) {
                        ForEach(rows.indices, id: \.self) { index in
                            let process = rows[index]
                            let share = switch desk.sort {
                            case .cpu: Double(process.cpu) / top
                            case .memory: Double(process.memoryBytes) / top
                            }
                            ProcessRow(process: process, share: min(max(share, 0), 1), memoryFirst: desk.sort == .memory)
                                .id(index)
                                .onTapGesture { desk.ask(process) }
                        }
                    }
                }
                .onChange(of: desk.sort) { reader.scrollTo(0, anchor: .top) }
            }
        }
    }

    private var sortButtons: some View {
        HStack(spacing: 0) {
            ForEach([(Sort.cpu, "CPU"), (Sort.memory, "메모리")], id: \.1) { value, label in
                let on = value == desk.sort
                Text(label)
                    .font(.system(size: 13, weight: on ? .medium : .regular))
                    .foregroundStyle(on ? palette.ground : palette.ink)
                    .padding(.horizontal, 16)
                    .padding(.vertical, 8)
                    .background(RoundedRectangle(cornerRadius: 11).fill(on ? palette.accent : .clear))
                    .contentShape(Rectangle())
                    .onTapGesture { desk.sortBy(value) }
            }
        }
        .padding(3)
        .background(RoundedRectangle(cornerRadius: 14).fill(palette.card))
    }
}

private struct ProcessRow: View {
    let process: ComputerProcess
    let share: Double
    var memoryFirst = false
    @Environment(\.palette) private var palette

    var body: some View {
        HStack(spacing: 0) {
            Text(process.name.first.map(String.init) ?? "")
                .font(.system(size: 11))
                .foregroundStyle(palette.muted)
                .frame(width: 24, height: 24)
                .background(RoundedRectangle(cornerRadius: 7).fill(palette.side))
            Text(process.name)
                .font(.system(size: 15))
                .foregroundStyle(palette.ink)
                .lineLimit(1)
                .truncationMode(.tail)
                .padding(.leading, 12)
                .padding(.trailing, 8)
                .frame(maxWidth: .infinity, alignment: .leading)
            // 고른 기준을 굵게 앞에, 다른 값은 작게 뒤에
            let cpu = String(format: "%.1f%%", process.cpu)
            let memory = memoryText(process.memoryBytes)
            Text(memoryFirst ? memory : cpu)
                .font(.system(size: 15, weight: .medium))
                .foregroundStyle(palette.ink)
                .frame(width: 76, alignment: .trailing)
            Text(memoryFirst ? cpu : memory)
                .font(.system(size: 12))
                .foregroundStyle(palette.muted)
                .frame(width: 68, alignment: .trailing)
        }
        .padding(.horizontal, 12)
        .frame(height: 44)
        .background(
            GeometryReader { proxy in
                ZStack(alignment: .leading) {
                    palette.card
                    palette.liquid.frame(width: proxy.size.width * share)
                }
            }
        )
        .clipShape(RoundedRectangle(cornerRadius: 14))
        .contentShape(Rectangle())
    }
}

/// 이보다 좁은 작은 칸은 글자를 위아래로 쌓는다.
let narrowTile = 96.0

/// 성능 작은 칸. CPU 와 메모리를 반씩, 각각 물결 위에 라벨(위)과 숫자(아래).
private struct PerfTile: View {
    let cpu: Float?
    let memory: Float?

    var body: some View {
        HStack(spacing: 2) {
            half("CPU", cpu)
            half("메모리", memory)
        }
        .clipShape(RoundedRectangle(cornerRadius: 20))
    }

    private func half(_ label: String, _ value: Float?) -> some View {
        MiniTile(background: nil) {
            Liquid(value: value)
        } first: { _ in
            MiniLabel(text: label)
        } second: { size in
            MiniNum(text: value.map { "\(Int($0))" } ?? "-", unit: "%", size: size)
        }
    }
}

/// 프로세스 작은 칸. CPU 가 가장 큰 프로그램 이름(위)과 CPU(아래)
private struct TopProcessTile: View {
    let top: ComputerProcess?

    var body: some View {
        MiniTile { size in
            MiniName(text: top?.name ?? "프로세스", size: size)
        } second: { _ in
            MiniLabel(text: top.map { String(format: "%.1f%%", $0.cpu) } ?? "-")
        }
    }
}

func rateText(_ bytes: Int64?) -> (String, String) {
    guard let bytes else { return ("-", "KB/s") }
    if bytes >= 1_000_000 { return (String(format: "%.1f", Double(bytes) / 1_000_000), "MB/s") }
    return ("\(bytes / 1000)", "KB/s")
}

func memoryText(_ bytes: Int64) -> String {
    if bytes >= 1_000_000_000 { return String(format: "%.1f GB", Double(bytes) / 1_000_000_000) }
    return "\(bytes / 1_000_000) MB"
}

/// 밀기 한 번이 몇 칸인지. 왼쪽으로 60pt 넘게 밀면 다음(1), 오른쪽이면 앞(-1).
/// 세로가 더 크면 목록 스크롤이라 0.
func swipeStep(_ translation: CGSize) -> Int {
    guard abs(translation.width) > abs(translation.height) else { return 0 }
    if translation.width <= -60 { return 1 }
    if translation.width >= 60 { return -1 }
    return 0
}

/// 세로에서 작은 칸이 다섯 이상이면 위 셋, 아래 나머지
func tileRows(_ small: [Screen]) -> [[Screen]] {
    guard small.count > 4 else { return [small] }
    return stride(from: 0, to: small.count, by: 3).map { Array(small[$0 ..< min($0 + 3, small.count)]) }
}

/// 가로 화면의 옆 줄. 폭이 높이의 1.75배를 넘으면 두 줄(한 줄 폭 15%, 최소 106), 아니면 한 줄(폭 20%, 140~196).
func sideLayout(width: CGFloat, height: CGFloat) -> (columns: Int, width: CGFloat) {
    if width > height * 1.75 { return (2, max(106, width * 0.15) * 2 + 10) }
    return (1, min(196, max(140, width * 0.2)))
}

/// 옆 줄을 줄 단위로 나눈다. 두 줄이고 칸 수가 홀수면 첫 칸은 혼자 한 줄.
func sideRows(_ small: [Screen], columns: Int) -> [[Screen]] {
    guard columns > 1 else { return small.map { [$0] } }
    var rows: [[Screen]] = []
    var rest = small[...]
    if small.count % 2 == 1, let first = rest.popFirst() { rows.append([first]) }
    while !rest.isEmpty { rows.append(Array(rest.prefix(2))); rest = rest.dropFirst(2) }
    return rows
}

/// 화면 배율. 바형 폰 가로(914x411)를 1로 두고 넓이에 비례해 1~1.6배.
func zoomFor(width: CGFloat, height: CGFloat) -> CGFloat {
    let base: CGFloat = (914 * 411).squareRoot()
    return min(1.6, max(1, (max(width, 0) * max(height, 0)).squareRoot() / base))
}

/// 큰 화면일수록 안의 화면 전체(글자, 칸)를 같은 배율로 키운다.
struct Zoomed<Content: View>: View {
    var scale: (CGFloat, CGFloat) -> CGFloat = zoomFor
    @ViewBuilder var content: Content

    var body: some View {
        GeometryReader { outer in
            let zoom = scale(outer.size.width, outer.size.height)
            content
                .frame(width: outer.size.width / zoom, height: outer.size.height / zoom)
                .scaleEffect(zoom, anchor: .topLeading)
        }
    }
}
