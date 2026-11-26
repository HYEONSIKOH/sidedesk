package io.github.hyeonsikoh.sidedesk.desk

import androidx.compose.animation.core.LinearEasing
import io.github.hyeonsikoh.sidedesk.ui.Track
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.hyeonsikoh.sidedesk.link.ProcessInfo
import io.github.hyeonsikoh.sidedesk.ai.AiBig
import io.github.hyeonsikoh.sidedesk.ai.AiScreenState
import io.github.hyeonsikoh.sidedesk.ai.AiTile
import io.github.hyeonsikoh.sidedesk.ai.ForgeTab
import io.github.hyeonsikoh.sidedesk.clock.ClockState
import io.github.hyeonsikoh.sidedesk.rest.RestPanel
import io.github.hyeonsikoh.sidedesk.music.MusicActions
import io.github.hyeonsikoh.sidedesk.music.MusicBig
import io.github.hyeonsikoh.sidedesk.music.MusicState
import io.github.hyeonsikoh.sidedesk.music.MusicTile
import io.github.hyeonsikoh.sidedesk.music.rememberTicks
import io.github.hyeonsikoh.sidedesk.shortcut.ShortcutBig
import io.github.hyeonsikoh.sidedesk.shortcut.ShortcutState
import io.github.hyeonsikoh.sidedesk.shortcut.ShortcutTile
import io.github.hyeonsikoh.sidedesk.clock.HomeBig
import io.github.hyeonsikoh.sidedesk.clock.HomeTile
import io.github.hyeonsikoh.sidedesk.clock.PickActions
import io.github.hyeonsikoh.sidedesk.clock.Picker
import io.github.hyeonsikoh.sidedesk.clock.rememberNow
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import io.github.hyeonsikoh.sidedesk.ui.Confirm
import io.github.hyeonsikoh.sidedesk.ui.ConfirmModal
import io.github.hyeonsikoh.sidedesk.ui.Palette
import io.github.hyeonsikoh.sidedesk.ui.fit
import io.github.hyeonsikoh.sidedesk.ui.longPressAnywhere
import io.github.hyeonsikoh.sidedesk.ui.behindModal
import kotlinx.coroutines.delay
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import kotlin.math.PI
import kotlin.math.sin

class DeskActions(
    val show: (Screen) -> Unit,
    val next: () -> Unit,
    val previous: () -> Unit,
    val sortBy: (Sort) -> Unit,
    val ask: (ProcessInfo) -> Unit,
    val cancel: () -> Unit,
    val confirmKill: () -> Unit,
    val dismissNotice: () -> Unit,
)

/** AI 세션 쪽 상태와 할 일 */
class AiPart(val state: AiScreenState, val selectTab: (ForgeTab) -> Unit) {
    companion object {
        val EMPTY = AiPart(AiScreenState()) {}
    }
}

/** 단축키 쪽 상태와 할 일 */
class ShortcutPart(val state: ShortcutState, val press: (Int) -> Unit) {
    companion object {
        val EMPTY = ShortcutPart(ShortcutState()) {}
    }
}

/** 음악 쪽 상태와 할 일. clock 은 진행 바를 움직이는 단조 시각(밀리초) */
class MusicPart(val state: MusicState, val clock: () -> Long, val actions: MusicActions) {
    companion object {
        val EMPTY = MusicPart(MusicState(), { 0L }, MusicActions({}, {}, {}, {}, {}))
    }
}

/** 시계 쪽 상태와 할 일. 홈 큰 화면, 홈 작은 칸, 고르기에 쓴다. */
class ClockPart(
    val state: ClockState,
    val now: () -> Long,
    val pick: () -> Unit,
    val nextFace: () -> Unit,
    val previousFace: () -> Unit,
    val actions: PickActions,
    /** 블루투스를 쓸 수 없는 이유. USB 는 된다. */
    val notice: String? = null,
    /** 고르기 판의 쉬기 설정 */
    val rest: RestPanel? = null,
    /** 고르기 판을 쉬기 설정부터 열지(그림 시험용) */
    val restOpen: Boolean = false,
)

@Composable
fun DeskScreen(
    state: DeskState,
    clock: ClockPart,
    actions: DeskActions,
    ai: AiPart = AiPart.EMPTY,
    shortcuts: ShortcutPart = ShortcutPart.EMPTY,
    music: MusicPart = MusicPart.EMPTY,
    modal: Confirm? = null,
    /** 안내 창 답. true 확인, false 취소 버튼, null 바깥 누르기나 뒤로 가기 */
    onModal: (Boolean?) -> Unit = {},
) {
    val shown = state.asking?.let(::killConfirm) ?: modal
    Box(Modifier.fillMaxSize().background(Palette.ground)) {
        Content(state, clock, ai, shortcuts, music, actions, Modifier.behindModal(shown != null))
        ConfirmModal(
            shown,
            onConfirm = { if (state.asking != null) actions.confirmKill() else onModal(true) },
            onDismiss = { if (state.asking != null) actions.cancel() else onModal(null) },
            onCancel = { if (state.asking != null) actions.cancel() else onModal(false) },
        )
    }
}

private fun killConfirm(process: ProcessInfo) = Confirm(
    title = "${eul(process.name)} 끝낼까요?",
    description = "CPU %.1f%%, 메모리 %s\n저장하지 않은 내용은 사라질 수 있습니다".format(process.cpu, memory(process.memoryBytes)),
    confirmLabel = "끝내기",
    danger = true,
    glyph = process.name.take(1).ifEmpty { "!" },
)

@Composable
private fun Content(state: DeskState, clock: ClockPart, ai: AiPart, shortcuts: ShortcutPart, music: MusicPart, actions: DeskActions, modifier: Modifier) {
    Track("content")
    // 시각은 여기서 읽지 않는다. 쓰는 칸이 각자 읽어 그 칸만 다시 구성한다.
    val parts = remember(clock, ai, shortcuts, music) { Parts(clock, ai, shortcuts, music) }
    BoxWithConstraints(modifier.fillMaxSize().safeDrawingPadding().padding(12.dp)) {
        val wide = maxWidth >= maxHeight
        if (wide) {
            Row(Modifier.fillMaxSize(), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Big(state, parts, actions, Modifier.weight(1f).fillMaxHeight())
                Column(Modifier.width(132.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    SmallTiles(state, state.small, parts, actions, Modifier.fillMaxWidth().weight(1f, fill = false).height(72.dp))
                }
            }
        } else {
            Column(Modifier.fillMaxSize(), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                // 작은 칸이 다섯이면 한 줄에 넣기에 좁아 두 줄(위 셋, 아래 둘)로 나눈다.
                val rows = if (state.small.size > 4) state.small.chunked(3) else listOf(state.small)
                rows.forEach { row ->
                    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                        SmallTiles(state, row, parts, actions, Modifier.weight(1f).height(72.dp))
                    }
                }
                Big(state, parts, actions, Modifier.weight(1f).fillMaxWidth())
            }
        }
    }
}

/** 이보다 좁은 작은 칸은 글자를 위아래로 쌓는다. */
internal val NARROW = 96.dp

/** 큰 화면 판. 시험에서 찾는다. */
const val BIG = "big"

/** 끊기면 컴퓨터에서 온 값만 흐리게 한다. 시계는 흐리지 않는다. */
private fun Modifier.dim(dimmed: Boolean) = alpha(if (dimmed) 0.4f else 1f)

private class Parts(val clock: ClockPart, val ai: AiPart, val shortcuts: ShortcutPart, val music: MusicPart)

// 시각을 읽는 칸은 따로 감싸, 시각이 바뀌면 감싼 칸만 다시 구성되게 한다.

@Composable
private fun HomeBigNow(clock: ClockPart) = HomeBig(clock.state, rememberNow(clock.now))

@Composable
private fun HomeTileNow(clock: ClockPart, modifier: Modifier) = HomeTile(clock.state, rememberNow(clock.now), modifier)

@Composable
private fun AiBigNow(ai: AiPart, time: () -> Long) = AiBig(ai.state, rememberNow(time), ai.selectTab)

@Composable
private fun MusicBigNow(music: MusicPart) =
    MusicBig(music.state, rememberTicks(music.clock, music.state.playing), music.actions)

@Composable
private fun MusicTileNow(music: MusicPart, modifier: Modifier) =
    MusicTile(music.state, rememberTicks(music.clock, music.state.playing), music.actions, modifier)

@Composable
private fun SmallTiles(state: DeskState, screens: List<Screen>, parts: Parts, actions: DeskActions, tile: Modifier) {
    Track("small-tiles")
    val clock = parts.clock
    val ai = parts.ai
    val shortcuts = parts.shortcuts
    // 고르는 중에 작은 칸을 누르면 고르기를 닫고 그 화면으로 간다.
    // 칸마다 누름 Modifier 를 한 번만 만든다. 값이 바뀌어 여기가 다시 구성되어도 같은 Modifier 라 다른 칸은 건너뛴다.
    val done = clock.actions.done
    val show = actions.show
    val press = remember(tile, done, show) {
        Screen.entries.associateWith { screen -> tile.clickable { done(); show(screen) } }
    }
    screens.forEach { screen ->
        val pressed = press.getValue(screen)
        when (screen) {
            Screen.Home -> HomeTileNow(clock, pressed)
            Screen.Ai -> AiTile(ai.state, pressed)
            Screen.Perf -> CpuTile(state.perf?.cpu, pressed.dim(state.dimmed))
            Screen.Processes -> TopProcessTile(state.rows.firstOrNull(), pressed.dim(state.dimmed))
            Screen.Shortcuts -> ShortcutTile(shortcuts.state, pressed)
            Screen.Music -> MusicTileNow(parts.music, pressed)
        }
    }
}

@Composable
private fun Big(state: DeskState, parts: Parts, actions: DeskActions, modifier: Modifier) {
    Track("big")
    val clock = parts.clock
    val ai = parts.ai
    val shortcuts = parts.shortcuts
    val home = state.big == Screen.Home
    val picking = home && clock.state.picking
    val swipe = with(LocalDensity.current) { 60.dp.toPx() }
    // 고르는 중 뒤로 가기는 고르기만 닫는다.
    BackHandler(enabled = picking) { clock.actions.done() }
    Box(
        modifier
            .testTag(BIG)
            .clip(RoundedCornerShape(28.dp))
            .background(Palette.panel)
            // 옆으로 밀면 다음 화면, 고르는 중이면 다음 모양. 밀기가 시작되면 길게 누르기는 취소된다.
            .pointerInput(picking) {
                var moved = 0f
                detectHorizontalDragGestures(
                    onDragStart = { moved = 0f },
                    onDragEnd = {
                        if (moved <= -swipe) if (picking) clock.nextFace() else actions.next()
                        if (moved >= swipe) if (picking) clock.previousFace() else actions.previous()
                    },
                ) { _, amount -> moved += amount }
            }
            .then(if (home) Modifier.longPressAnywhere { clock.pick() } else Modifier)
            .padding(14.dp),
    ) {
        when (state.big) {
            Screen.Home -> HomeBigNow(clock)
            Screen.Ai -> AiBigNow(ai, clock.now)
            Screen.Perf -> PerfScreen(state, Modifier.dim(state.dimmed))
            Screen.Processes -> ProcessScreen(state, actions, Modifier.dim(state.dimmed))
            Screen.Shortcuts -> ShortcutBig(shortcuts.state, shortcuts.press)
            Screen.Music -> MusicBigNow(parts.music)
        }
        if (home && !picking) {
            clock.notice?.let {
                Text(it, color = Palette.muted, fontSize = 12.sp, textAlign = TextAlign.Center, modifier = Modifier.align(Alignment.TopCenter))
            }
        }
        if (picking) Picker(clock.state, clock.actions, Modifier.align(Alignment.BottomCenter).fillMaxWidth(), clock.rest, clock.restOpen)
        state.notice?.let { notice ->
            LaunchedEffect(notice) {
                delay(4_000)
                actions.dismissNotice()
            }
            Text(
                notice,
                color = Palette.ground,
                fontSize = 14.sp,
                modifier = Modifier
                    // 고르기 줄과 겹치지 않게 고르는 중에는 위에 띄운다.
                    .align(if (picking) Alignment.TopCenter else Alignment.BottomCenter)
                    .padding(vertical = 8.dp)
                    .clip(RoundedCornerShape(20.dp))
                    .background(Palette.ink)
                    .padding(horizontal = 18.dp, vertical = 10.dp),
            )
        }
    }
}

@Composable
private fun PerfScreen(state: DeskState, modifier: Modifier) {
    Track("perf-big")
    val perf = state.perf
    Column(modifier.fillMaxSize(), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Row(Modifier.weight(1f).fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            val gauge = Modifier.weight(1f).fillMaxHeight()
            Gauge("CPU", perf?.cpu, gauge)
            Gauge("GPU", perf?.gpu, gauge)
            Gauge("메모리", perf?.memory, gauge)
            Gauge("디스크", perf?.disk, gauge)
        }
        Row(
            Modifier.fillMaxWidth().height(56.dp).clip(RoundedCornerShape(18.dp)).background(Palette.card)
                .padding(horizontal = 16.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Rate("↓", perf?.receivePerSecond)
            NetGraph(state.received, state.sent, Modifier.weight(1f).fillMaxHeight().padding(horizontal = 14.dp, vertical = 10.dp))
            Rate("↑", perf?.sendPerSecond)
        }
    }
}

@Composable
private fun Gauge(label: String, value: Float?, modifier: Modifier) {
    BoxWithConstraints(modifier.clip(RoundedCornerShape(22.dp)).background(Palette.card)) {
        // 좁은 칸에서도 숫자와 % 가 한 줄에 들어가게 칸 너비에 맞춘다.
        val inner = maxWidth.value - 28f
        val size = fit(minOf(38f, inner / 1.6f).dp)
        val labelSize = fit(minOf(14f, inner / 3f).dp)
        Liquid(value, Modifier.fillMaxSize())
        Column(Modifier.padding(14.dp)) {
            Text(label, color = Palette.ink, fontSize = labelSize, maxLines = 1, softWrap = false)
            Percent(value, size)
        }
    }
}

/** 값만큼 아래에서 차오르는 물결 */
@Composable
private fun Liquid(value: Float?, modifier: Modifier) {
    // 쉬지 않고 도는 애니메이션은 두지 않는다. 값이 바뀔 때만 0.6초 동안 높이를 옮기고 물결을 1/4 바퀴 민다.
    val level by animateFloatAsState((value ?: 0f).coerceIn(0f, 100f) / 100f, tween(600), label = "level")
    var turns by remember { mutableIntStateOf(0) }
    var seen by remember { mutableStateOf(value) }
    LaunchedEffect(value) {
        if (value != seen) turns += 1
        seen = value
    }
    val phase by animateFloatAsState(turns * (PI / 2).toFloat(), tween(600, easing = LinearEasing), label = "phase")
    val liquid = Palette.liquid
    Canvas(modifier) {
        if (level <= 0f) return@Canvas
        val top = size.height * (1f - level)
        val amp = 4.dp.toPx()
        val path = Path().apply {
            moveTo(0f, size.height)
            var x = 0f
            while (x <= size.width) {
                lineTo(x, top + amp * sin(x / size.width * 2 * PI.toFloat() + phase))
                x += 4f
            }
            lineTo(size.width, top + amp * sin(2 * PI.toFloat() + phase))
            lineTo(size.width, size.height)
            close()
        }
        drawPath(path, liquid)
    }
}

@Composable
private fun Percent(value: Float?, size: TextUnit, decimals: Boolean = false) {
    Text(
        buildAnnotatedString {
            append(if (value == null) "-" else if (decimals) "%.1f".format(value) else value.toInt().toString())
            withStyle(SpanStyle(fontSize = size * 0.45f)) { append("%") }
        },
        color = Palette.ink,
        fontSize = size,
        fontWeight = FontWeight.Medium,
        maxLines = 1,
        softWrap = false,
    )
}

@Composable
private fun Rate(arrow: String, bytes: Long?) {
    Text(
        buildAnnotatedString {
            append("$arrow  ")
            val (number, unit) = rate(bytes)
            withStyle(SpanStyle(fontSize = fit(20.dp), fontWeight = FontWeight.Medium)) { append(number) }
            withStyle(SpanStyle(fontSize = fit(12.dp))) { append(" $unit") }
        },
        color = Palette.ink,
        fontSize = fit(14.dp),
        maxLines = 1,
        softWrap = false,
    )
}

@Composable
private fun NetGraph(received: List<Long>, sent: List<Long>, modifier: Modifier) {
    val sentColor = Palette.muted
    val receivedColor = Palette.accent
    Canvas(modifier) {
        val top = maxOf(received.maxOrNull() ?: 0L, sent.maxOrNull() ?: 0L, 1L).toFloat()
        fun line(values: List<Long>, color: Color) {
            if (values.size < 2) return
            val step = size.width / (DeskViewModel.HISTORY - 1)
            val start = size.width - step * (values.size - 1)
            val path = Path()
            values.forEachIndexed { i, v ->
                val x = start + step * i
                val y = size.height * (1f - v / top)
                if (i == 0) path.moveTo(x, y) else path.lineTo(x, y)
            }
            drawPath(path, color, style = Stroke(1.5.dp.toPx(), cap = StrokeCap.Round, join = StrokeJoin.Round))
        }
        line(sent, sentColor)
        line(received, receivedColor)
    }
}

@Composable
private fun ProcessScreen(state: DeskState, actions: DeskActions, modifier: Modifier) {
    Track("processes-big")
    Column(modifier.fillMaxSize()) {
        Row(Modifier.fillMaxWidth().padding(start = 4.dp, bottom = 10.dp), verticalAlignment = Alignment.CenterVertically) {
            Percent(state.perf?.cpu, 34.sp)
            Text("CPU 사용률", color = Palette.ink, fontSize = 12.sp, modifier = Modifier.padding(start = 10.dp).weight(1f))
            SortButtons(state.sort, actions.sortBy)
        }
        val top = when (state.sort) {
            Sort.Cpu -> state.rows.maxOfOrNull { it.cpu.toDouble() }
            Sort.Memory -> state.rows.maxOfOrNull { it.memoryBytes.toDouble() }
        }?.takeIf { it > 0 } ?: 1.0
        // 키를 붙이면 순서가 바뀔 때 목록이 그 줄을 따라 내려가므로 자리 번호로 둔다.
        val list = rememberLazyListState()
        LaunchedEffect(state.sort) { list.scrollToItem(0) }
        LazyColumn(state = list, verticalArrangement = Arrangement.spacedBy(8.dp)) {
            items(state.rows) { process ->
                val share = when (state.sort) {
                    Sort.Cpu -> process.cpu / top
                    Sort.Memory -> process.memoryBytes / top
                }.toFloat().coerceIn(0f, 1f)
                ProcessRow(process, share) { actions.ask(process) }
            }
        }
    }
}

@Composable
private fun SortButtons(sort: Sort, onSort: (Sort) -> Unit) {
    Row(Modifier.clip(RoundedCornerShape(14.dp)).background(Palette.card).padding(3.dp)) {
        listOf(Sort.Cpu to "CPU", Sort.Memory to "메모리").forEach { (value, label) ->
            val on = value == sort
            Text(
                label,
                color = if (on) Palette.ground else Palette.ink,
                fontSize = 13.sp,
                fontWeight = if (on) FontWeight.Medium else FontWeight.Normal,
                modifier = Modifier
                    .clip(RoundedCornerShape(11.dp))
                    .background(if (on) Palette.accent else Color.Transparent)
                    .clickable { onSort(value) }
                    .padding(horizontal = 16.dp, vertical = 8.dp),
            )
        }
    }
}

@Composable
private fun ProcessRow(process: ProcessInfo, share: Float, onClick: () -> Unit) {
    Box(Modifier.fillMaxWidth().height(44.dp).clip(RoundedCornerShape(14.dp)).background(Palette.card).clickable(onClick = onClick)) {
        Box(Modifier.fillMaxHeight().fillMaxWidth(share).background(Palette.liquid))
        Row(Modifier.fillMaxSize().padding(horizontal = 12.dp), verticalAlignment = Alignment.CenterVertically) {
            Box(
                Modifier.size(24.dp).clip(RoundedCornerShape(7.dp)).background(Palette.side),
                contentAlignment = Alignment.Center,
            ) {
                Text(process.name.take(1), color = Palette.muted, fontSize = 11.sp)
            }
            Text(
                process.name,
                color = Palette.ink,
                fontSize = 15.sp,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.padding(start = 12.dp, end = 8.dp).weight(1f),
            )
            Text(
                "%.1f%%".format(process.cpu),
                color = Palette.ink,
                fontSize = 15.sp,
                fontWeight = FontWeight.Medium,
                textAlign = TextAlign.End,
                modifier = Modifier.width(60.dp),
            )
            Text(
                memory(process.memoryBytes),
                color = Palette.ink,
                fontSize = 12.sp,
                textAlign = TextAlign.End,
                modifier = Modifier.width(76.dp),
            )
        }
    }
}

@Composable
private fun CpuTile(cpu: Float?, modifier: Modifier) {
    Track("cpu-tile")
    BoxWithConstraints(modifier.clip(RoundedCornerShape(20.dp)).background(Palette.side)) {
        Liquid(cpu, Modifier.fillMaxSize())
        if (maxWidth < NARROW) {
            // 좁은 세로에서 작은 칸이 넷이면 칸이 72dp 남짓이라 글자를 위아래로 쌓는다.
            Column(Modifier.fillMaxSize(), verticalArrangement = Arrangement.Center, horizontalAlignment = Alignment.CenterHorizontally) {
                Text("CPU", color = Palette.ink, fontSize = fit(11.dp), maxLines = 1)
                Percent(cpu, fit(20.dp))
            }
        } else {
            Row(Modifier.fillMaxSize().padding(horizontal = 14.dp), verticalAlignment = Alignment.CenterVertically) {
                Text("CPU", color = Palette.ink, fontSize = fit(12.dp), maxLines = 1)
                Spacer(Modifier.weight(1f))
                Percent(cpu, fit(26.dp))
            }
        }
    }
}

@Composable
private fun TopProcessTile(top: ProcessInfo?, modifier: Modifier) {
    Track("process-tile")
    BoxWithConstraints(modifier.clip(RoundedCornerShape(20.dp)).background(Palette.side)) {
        val narrow = maxWidth < NARROW
        Column(
            Modifier.fillMaxSize().padding(horizontal = if (narrow) 8.dp else 14.dp),
            verticalArrangement = Arrangement.Center,
            horizontalAlignment = if (narrow) Alignment.CenterHorizontally else Alignment.Start,
        ) {
            Text(
                top?.name ?: "프로세스",
                color = Palette.ink,
                fontSize = fit(if (narrow) 12.dp else 14.dp),
                fontWeight = FontWeight.Medium,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(top?.let { "%.1f%%".format(it.cpu) } ?: "-", color = Palette.muted, fontSize = fit(if (narrow) 11.dp else 12.dp), maxLines = 1)
        }
    }
}

private fun rate(bytes: Long?): Pair<String, String> = when {
    bytes == null -> "-" to "KB/s"
    bytes >= 1_000_000 -> "%.1f".format(bytes / 1_000_000.0) to "MB/s"
    else -> (bytes / 1_000).toString() to "KB/s"
}

private fun memory(bytes: Long): String = when {
    bytes >= 1_000_000_000 -> "%.1f GB".format(bytes / 1_000_000_000.0)
    else -> "${bytes / 1_000_000} MB"
}
