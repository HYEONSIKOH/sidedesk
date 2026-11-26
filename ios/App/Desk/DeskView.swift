import SwiftUI

/// 왼쪽(세로면 아래) 큰 화면과 작은 칸. 확인 창은 공통 `ConfirmModal` 하나.
struct DeskView: View {
    let desk: DeskModel
    let clock: ClockModel
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
            TimelineView(.periodic(from: .now, by: 1)) { context in
                content(now: context.date)
            }
            .behindModal(shown != nil)
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

    private func content(now: Date) -> some View {
        GeometryReader { proxy in
            let wide = proxy.size.width >= proxy.size.height
            Group {
                if wide {
                    HStack(spacing: 12) {
                        big(now: now)
                        VStack(spacing: 10) {
                            tiles(now: now).frame(height: 72)
                        }
                        .frame(width: 132)
                        .frame(maxHeight: .infinity, alignment: .top)
                    }
                } else {
                    VStack(spacing: 12) {
                        HStack(spacing: 10) {
                            tiles(now: now).frame(maxWidth: .infinity).frame(height: 72)
                        }
                        big(now: now)
                    }
                }
            }
            .padding(12)
        }
    }

    @ViewBuilder
    private func tiles(now: Date) -> some View {
        // 고르는 중에 작은 칸을 누르면 고르기를 닫고 그 화면으로 간다.
        ForEach(desk.small, id: \.self) { screen in
            Group {
                switch screen {
                case .home: HomeTile(clock: clock, now: now)
                case .perf: CpuTile(cpu: desk.perf?.cpu).dim(desk.dimmed)
                case .processes: TopProcessTile(top: desk.rows.first).dim(desk.dimmed)
                }
            }
            .contentShape(Rectangle())
            .onTapGesture {
                clock.done()
                desk.show(screen)
            }
        }
    }

    private func big(now: Date) -> some View {
        let home = desk.big == .home
        let picking = home && clock.picking
        return ZStack {
            switch desk.big {
            case .home:
                HomeBig(clock: clock, now: now) {
                    // 길게 눌러 고르는 중이면 출처 링크를 열지 않는다.
                    if !clock.picking, let url = URL(string: "https://open-meteo.com/") {
                        UIApplication.shared.open(url)
                    }
                }
            case .perf:
                PerfScreen(desk: desk).dim(desk.dimmed)
            case .processes:
                ProcessScreen(desk: desk).dim(desk.dimmed)
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
                    FacePicker(clock: clock)
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
                let moved = drag.translation.width
                guard abs(moved) > abs(drag.translation.height) else { return }
                if moved <= -60 { picking ? clock.nextFace() : desk.next() }
                if moved >= 60 { picking ? clock.previousFace() : desk.previous() }
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

/// 값만큼 아래에서 차오르는 물결
private struct Liquid: View {
    let value: Float?
    @Environment(\.palette) private var palette

    var body: some View {
        let level = Double(min(max(value ?? 0, 0), 100)) / 100
        TimelineView(.animation(minimumInterval: 1 / 30)) { context in
            let phase = context.date.timeIntervalSinceReferenceDate.truncatingRemainder(dividingBy: 4) / 4 * 2 * .pi
            Canvas { canvas, size in
                guard level > 0 else { return }
                let top = size.height * (1 - level)
                let amp = 4.0
                var path = Path()
                path.move(to: CGPoint(x: 0, y: size.height))
                var x = 0.0
                while x <= size.width {
                    path.addLine(to: CGPoint(x: x, y: top + amp * sin(x / size.width * 2 * .pi + phase)))
                    x += 4
                }
                path.addLine(to: CGPoint(x: size.width, y: top + amp * sin(2 * .pi + phase)))
                path.addLine(to: CGPoint(x: size.width, y: size.height))
                path.closeSubpath()
                canvas.fill(path, with: .color(palette.liquid))
            }
        }
        .animation(.easeOut(duration: 0.6), value: level)
    }
}

private struct Percent: View {
    let value: Float?
    let size: Double
    @Environment(\.palette) private var palette

    var body: some View {
        (Text(value.map { "\(Int($0))" } ?? "-").font(.system(size: size, weight: .medium))
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
                Percent(value: desk.perf?.cpu, size: 34)
                Text("CPU 사용률").font(.system(size: 12)).foregroundStyle(palette.ink).padding(.leading, 10)
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
                            ProcessRow(process: process, share: min(max(share, 0), 1))
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
            Text(String(format: "%.1f%%", process.cpu))
                .font(.system(size: 15, weight: .medium))
                .foregroundStyle(palette.ink)
                .frame(width: 60, alignment: .trailing)
            Text(memoryText(process.memoryBytes))
                .font(.system(size: 12))
                .foregroundStyle(palette.ink)
                .frame(width: 76, alignment: .trailing)
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

private struct CpuTile: View {
    let cpu: Float?
    @Environment(\.palette) private var palette

    var body: some View {
        ZStack {
            Liquid(value: cpu)
            HStack {
                Text("CPU").font(.system(size: 12)).foregroundStyle(palette.ink)
                Spacer()
                Percent(value: cpu, size: 26)
            }
            .padding(.horizontal, 14)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(RoundedRectangle(cornerRadius: 20).fill(palette.side))
        .clipShape(RoundedRectangle(cornerRadius: 20))
    }
}

private struct TopProcessTile: View {
    let top: ComputerProcess?
    @Environment(\.palette) private var palette

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(top?.name ?? "프로세스").font(.system(size: 14, weight: .medium)).foregroundStyle(palette.ink).lineLimit(1)
            Text(top.map { String(format: "%.1f%%", $0.cpu) } ?? "-").font(.system(size: 12)).foregroundStyle(palette.muted)
        }
        .padding(.horizontal, 14)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 20).fill(palette.side))
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
