import SwiftUI
import UIKit

/// 진행 바가 쓰는 단조 시각. 재생 중이고 보일 때만 0.5초마다 다시 읽는다(멈췄으면 읽지 않는다).
private struct Ticking<Content: View>: View {
    let playing: Bool
    @ViewBuilder let content: (Int64) -> Content

    var body: some View {
        if playing {
            TimelineView(MusicTicks()) { _ in content(monotonicMillis()) }
        } else {
            content(monotonicMillis())
        }
    }
}

/// 음악 큰 화면(시안 다섯째 그림). 가로는 왼쪽 그림, 세로는 위 그림
struct MusicBig: View {
    let music: MusicModel

    var body: some View {
        GeometryReader { proxy in
            let w = proxy.size.width
            let h = proxy.size.height
            let wide = w > h
            let art = wide ? min(h - 16, w * 0.42) : min(w * 0.72, h * 0.42)
            ZStack(alignment: .bottom) {
                Group {
                    if wide {
                        HStack(spacing: 24) {
                            Artwork(jpeg: music.art, size: art)
                            Info(music: music)
                        }
                    } else {
                        VStack(spacing: 18) {
                            Artwork(jpeg: music.art, size: art)
                            Info(music: music)
                        }
                    }
                }
                .padding(8)
                .frame(width: w, height: h)
                .dim(music.dimmed)
                if let notice = music.notice {
                    NoticePill(text: notice)
                }
            }
        }
    }
}

/// jpeg 를 백그라운드에서 푼다. 깨졌으면 기본 그림이 나온다.
private struct Artwork: View {
    let jpeg: Data?
    let size: Double
    @State private var image: UIImage?

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: size * 0.1)
        Group {
            if let image {
                Image(uiImage: image).resizable().scaledToFill()
            } else {
                DefaultArt()
            }
        }
        .frame(width: size, height: size)
        .clipShape(shape)
        .task(id: jpeg) {
            guard let jpeg else {
                image = nil
                return
            }
            let decoded = await Task.detached(priority: .utility) { UIImage(data: jpeg)?.preparingForDisplay() }.value
            image = decoded
        }
    }
}

/// 그림이 없을 때의 기본 그림. 시안의 하늘, 해, 물결
struct DefaultArt: View {
    var body: some View {
        Canvas { context, size in
            context.fill(Path(CGRect(origin: .zero, size: size)), with: .color(Color(hex: 0x7FA0B4)))
            let sun = min(size.width, size.height) * 0.15
            context.fill(Path(ellipseIn: CGRect(x: size.width * 0.7 - sun, y: size.height * 0.3 - sun, width: sun * 2, height: sun * 2)), with: .color(Color(hex: 0xF3DFA8)))
            func wave(_ top: Double, _ color: Color) {
                var path = Path()
                path.move(to: CGPoint(x: 0, y: size.height * top))
                path.addCurve(
                    to: CGPoint(x: size.width, y: size.height * (top - 0.04)),
                    control1: CGPoint(x: size.width * 0.3, y: size.height * (top - 0.1)),
                    control2: CGPoint(x: size.width * 0.6, y: size.height * (top + 0.08))
                )
                path.addLine(to: CGPoint(x: size.width, y: size.height))
                path.addLine(to: CGPoint(x: 0, y: size.height))
                path.closeSubpath()
                context.fill(path, with: .color(color))
            }
            wave(0.6, Color(hex: 0x5A7E94))
            wave(0.74, Color(hex: 0x3E6075))
        }
    }
}

private struct Info: View {
    let music: MusicModel
    @Environment(\.palette) private var palette

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if music.hasTrack, let now = music.now {
                Text(playerName(now.player)).font(.system(size: 13)).foregroundStyle(palette.muted).lineLimit(1)
                Text(now.title).font(.system(size: 22, weight: .semibold)).foregroundStyle(palette.ink).lineLimit(2)
                if !now.artist.trimmingCharacters(in: .whitespaces).isEmpty {
                    Text(now.artist).font(.system(size: 14)).foregroundStyle(palette.muted).lineLimit(1)
                }
                Ticking(playing: now.playing && !music.dimmed) { ms in
                    Progress(now: now, nowMs: ms, lostAt: music.lostAt)
                }
                .padding(.top, 14)
            } else {
                Text("재생 중인 곡 없음").font(.system(size: 18)).foregroundStyle(palette.muted).lineLimit(1)
                Spacer().frame(height: 14)
            }
            Controls(music: music).padding(.top, 10)
            if let volume = music.volume, volume.available {
                VolumeBar(music: music, volume: volume).padding(.top, 14)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

private struct Progress: View {
    let now: NowPlaying
    let nowMs: Int64
    let lostAt: Int64?
    @Environment(\.palette) private var palette

    var body: some View {
        let position = positionAt(now, nowMs, lostAt: lostAt)
        if now.durationMs > 0 {
            VStack(spacing: 6) {
                Bar(fraction: Double(position) / Double(now.durationMs)).frame(height: 6)
                HStack {
                    Text(clockText(position))
                    Spacer()
                    Text(clockText(now.durationMs))
                }
                .font(.system(size: 12))
                .foregroundStyle(palette.muted)
            }
        } else {
            // 전체 길이를 모르면 막대 없이 지난 시간만
            Text(clockText(position)).font(.system(size: 12)).foregroundStyle(palette.muted)
        }
    }
}

struct Bar: View {
    let fraction: Double
    @Environment(\.palette) private var palette

    var body: some View {
        GeometryReader { proxy in
            ZStack(alignment: .leading) {
                palette.track
                palette.accent.frame(width: proxy.size.width * min(max(fraction.isFinite ? fraction : 0, 0), 1))
            }
        }
        .clipShape(Capsule())
    }
}

private struct Controls: View {
    let music: MusicModel

    var body: some View {
        let now = music.now
        let live = music.hasTrack && !music.dimmed
        HStack {
            Spacer()
            SkipButton(back: true, enabled: live && now?.canPrevious == true, action: music.previous)
            Spacer()
            PlayButton(playing: music.playing, enabled: live && now?.canPlay == true, diameter: 64, action: music.playPause)
            Spacer()
            SkipButton(back: false, enabled: live && now?.canNext == true, action: music.next)
            Spacer()
        }
    }
}

/// 재생과 멈춤 그림은 돌며 바뀐다(시안 .ppi).
struct PlayButton: View {
    let playing: Bool
    let enabled: Bool
    let diameter: Double
    let action: () -> Void
    var filled = true
    @Environment(\.palette) private var palette

    var body: some View {
        let fade = enabled ? 1 : dimAlpha
        let ink = (filled ? palette.panel : palette.ink).opacity(fade)
        Button(action: action) {
            ZStack {
                Circle().fill((filled ? palette.accent : palette.card).opacity(fade)).frame(width: diameter, height: diameter)
                Canvas { context, size in
                    if playing {
                        let bar = size.width * 0.28
                        context.fill(Path(CGRect(x: size.width * 0.12, y: 0, width: bar, height: size.height)), with: .color(ink))
                        context.fill(Path(CGRect(x: size.width * 0.6, y: 0, width: bar, height: size.height)), with: .color(ink))
                    } else {
                        var path = Path()
                        path.move(to: CGPoint(x: size.width * 0.15, y: 0))
                        path.addLine(to: CGPoint(x: size.width, y: size.height / 2))
                        path.addLine(to: CGPoint(x: size.width * 0.15, y: size.height))
                        path.closeSubpath()
                        context.fill(path, with: .color(ink))
                    }
                }
                .frame(width: diameter * 0.4, height: diameter * 0.4)
                .animation(.easeOut(duration: 0.25), value: playing)
            }
            .frame(width: max(diameter, 48), height: max(diameter, 48))
            .contentShape(Circle())
        }
        .buttonStyle(.plain)
        .disabled(!enabled)
        .accessibilityLabel("재생 또는 멈춤")
    }
}

private struct SkipButton: View {
    let back: Bool
    let enabled: Bool
    let action: () -> Void
    @Environment(\.palette) private var palette

    var body: some View {
        let ink = palette.ink.opacity(enabled ? 1 : dimAlpha)
        Button(action: action) {
            Canvas { context, size in
                var path = Path()
                path.move(to: .zero)
                path.addLine(to: CGPoint(x: size.width * 0.78, y: size.height / 2))
                path.addLine(to: CGPoint(x: 0, y: size.height))
                path.closeSubpath()
                context.fill(path, with: .color(ink))
                context.fill(Path(CGRect(x: size.width * 0.82, y: 0, width: size.width * 0.16, height: size.height)), with: .color(ink))
            }
            .frame(width: 22, height: 22)
            .rotationEffect(.degrees(back ? 180 : 0))
            .frame(width: 48, height: 48)
            .contentShape(Circle())
        }
        .buttonStyle(.plain)
        .disabled(!enabled)
        .accessibilityLabel(back ? "이전 곡" : "다음 곡")
    }
}

private struct VolumeBar: View {
    let music: MusicModel
    let volume: Volume
    @Environment(\.palette) private var palette

    var body: some View {
        let usable = volume.settable && !music.dimmed
        HStack(spacing: 10) {
            Canvas { context, size in
                let style = StrokeStyle(lineWidth: min(size.width, size.height) * 0.09, lineCap: .round, lineJoin: .round)
                var body = Path()
                body.addLines([
                    CGPoint(x: size.width * 0.1, y: size.height * 0.38), CGPoint(x: size.width * 0.3, y: size.height * 0.38),
                    CGPoint(x: size.width * 0.55, y: size.height * 0.15), CGPoint(x: size.width * 0.55, y: size.height * 0.85),
                    CGPoint(x: size.width * 0.3, y: size.height * 0.62), CGPoint(x: size.width * 0.1, y: size.height * 0.62),
                ])
                body.closeSubpath()
                context.stroke(body, with: .color(palette.ink), style: style)
                var mark = Path()
                if volume.muted {
                    mark.move(to: CGPoint(x: size.width * 0.68, y: size.height * 0.35))
                    mark.addLine(to: CGPoint(x: size.width * 0.95, y: size.height * 0.65))
                    mark.move(to: CGPoint(x: size.width * 0.95, y: size.height * 0.35))
                    mark.addLine(to: CGPoint(x: size.width * 0.68, y: size.height * 0.65))
                } else {
                    mark.addArc(center: CGPoint(x: size.width * 0.65, y: size.height * 0.5), radius: size.width * 0.15, startAngle: .degrees(-45), endAngle: .degrees(45), clockwise: false)
                }
                context.stroke(mark, with: .color(palette.ink), style: style)
            }
            .frame(width: 20, height: 20)
            GeometryReader { proxy in
                Bar(fraction: Double(music.level ?? 0) / 100)
                    .frame(height: 6)
                    .frame(maxHeight: .infinity)
                    .contentShape(Rectangle())
                    .gesture(
                        DragGesture(minimumDistance: 0)
                            .onChanged { drag in music.dragVolume(Int(drag.location.x / proxy.size.width * 100)) }
                            .onEnded { _ in music.releaseVolume() },
                        isEnabled: usable
                    )
            }
            .frame(height: 28)
            .opacity(usable && !volume.muted ? 1 : dimAlpha)
        }
    }
}

/// 음악 작은 칸(시안 .mn, .edge). 왼쪽 버튼은 재생/멈춤이고, 칸의 나머지는 음악 화면으로 간다.
struct MusicTile: View {
    let music: MusicModel
    @Environment(\.palette) private var palette

    var body: some View {
        GeometryReader { proxy in
            let narrow = proxy.size.width < 96
            ZStack(alignment: .bottom) {
                if music.hasTrack, let now = music.now {
                    HStack(spacing: 0) {
                        PlayButton(playing: music.playing, enabled: !music.dimmed && now.canPlay, diameter: 30, action: music.playPause, filled: false)
                        if !narrow {
                            Text(now.title)
                                .font(.system(size: 13, weight: .medium))
                                .foregroundStyle(palette.ink)
                                .lineLimit(1)
                                .padding(.trailing, 8)
                                .frame(maxWidth: .infinity, alignment: .leading)
                        }
                    }
                    .padding(.horizontal, 4)
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: narrow ? .center : .leading)
                    .dim(music.dimmed)
                    if now.durationMs > 0 {
                        Ticking(playing: now.playing && !music.dimmed) { ms in
                            Bar(fraction: Double(positionAt(now, ms, lostAt: music.lostAt)) / Double(now.durationMs))
                        }
                        .frame(height: 3)
                        .padding(.horizontal, 16)
                        .padding(.vertical, 8)
                    }
                } else {
                    Text("음악").font(.system(size: 14)).foregroundStyle(palette.ink).lineLimit(1)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                }
            }
        }
        .background(RoundedRectangle(cornerRadius: 20).fill(palette.side))
    }
}
