package io.github.hyeonsikoh.sidedesk.desk

import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
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
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
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
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import io.github.hyeonsikoh.sidedesk.link.ProcessInfo
import io.github.hyeonsikoh.sidedesk.link.Topic
import kotlinx.coroutines.delay
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import kotlin.math.PI
import kotlin.math.sin

private object Palette {
    val ground = Color(0xFF15181C)
    val panel = Color(0xFF22272E)
    val card = Color(0xFF2C323A)
    val side = Color(0xFF1D2127)
    val liquid = Color(0xFF33475A)
    val accent = Color(0xFF8FAECB)
    val ink = Color(0xFFDCE1E7)
    val muted = Color(0xFF8B939E)
    val danger = Color(0xFFE6A6A6)
}

class DeskActions(
    val show: (Topic) -> Unit,
    val sortBy: (Sort) -> Unit,
    val ask: (ProcessInfo) -> Unit,
    val cancel: () -> Unit,
    val confirmKill: () -> Unit,
    val dismissNotice: () -> Unit,
)

@Composable
fun DeskScreen(state: DeskState, actions: DeskActions) {
    BoxWithConstraints(Modifier.fillMaxSize().background(Palette.ground).safeDrawingPadding().padding(12.dp)) {
        val wide = maxWidth >= maxHeight
        val content = Modifier.alpha(if (state.dimmed) 0.4f else 1f)
        if (wide) {
            Row(content.fillMaxSize(), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Big(state, actions, Modifier.weight(1f).fillMaxHeight())
                Column(Modifier.width(132.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    SmallTiles(state, actions, Modifier.fillMaxWidth().height(72.dp))
                }
            }
        } else {
            Column(content.fillMaxSize(), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    SmallTiles(state, actions, Modifier.weight(1f).height(72.dp))
                }
                Big(state, actions, Modifier.weight(1f).fillMaxWidth())
            }
        }
    }
    state.asking?.let { process -> KillDialog(process, actions.cancel, actions.confirmKill) }
}

/** 칸과 같은 둥근 어두운 판에, 정렬 버튼과 같은 모양의 버튼 둘 */
@Composable
private fun KillDialog(process: ProcessInfo, onCancel: () -> Unit, onConfirm: () -> Unit) {
    Dialog(onDismissRequest = onCancel, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        Column(
            Modifier
                .padding(24.dp)
                .widthIn(max = 380.dp)
                .fillMaxWidth()
                .clip(RoundedCornerShape(22.dp))
                .background(Palette.card)
                .padding(20.dp),
        ) {
            Text("${eul(process.name)} 끝낼까요?", color = Palette.ink, fontSize = 18.sp, fontWeight = FontWeight.Medium)
            Text(
                "CPU %.1f%%, 메모리 %s".format(process.cpu, memory(process.memoryBytes)),
                color = Palette.muted,
                fontSize = 13.sp,
                modifier = Modifier.padding(top = 6.dp),
            )
            Row(Modifier.padding(top = 20.dp).fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                DialogButton("취소", Palette.side, Palette.ink, Modifier.weight(1f), onCancel)
                DialogButton("끝내기", Palette.danger, Palette.ground, Modifier.weight(1f), onConfirm)
            }
        }
    }
}

@Composable
private fun DialogButton(label: String, background: Color, ink: Color, modifier: Modifier, onClick: () -> Unit) {
    Box(
        modifier.height(44.dp).clip(RoundedCornerShape(14.dp)).background(background).clickable(onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        Text(label, color = ink, fontSize = 15.sp, fontWeight = FontWeight.Medium)
    }
}

@Composable
private fun SmallTiles(state: DeskState, actions: DeskActions, tile: Modifier) {
    ClockTile(tile)
    state.small.forEach { topic ->
        when (topic) {
            Topic.Perf -> CpuTile(state.perf?.cpu, tile.clickable { actions.show(Topic.Perf) })
            Topic.Processes -> TopProcessTile(state.rows.firstOrNull(), tile.clickable { actions.show(Topic.Processes) })
        }
    }
}

@Composable
private fun Big(state: DeskState, actions: DeskActions, modifier: Modifier) {
    Box(modifier.clip(RoundedCornerShape(28.dp)).background(Palette.panel).padding(14.dp)) {
        when (state.big) {
            Topic.Perf -> PerfScreen(state)
            Topic.Processes -> ProcessScreen(state, actions)
        }
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
                    .align(Alignment.BottomCenter)
                    .padding(bottom = 8.dp)
                    .clip(RoundedCornerShape(20.dp))
                    .background(Palette.ink)
                    .padding(horizontal = 18.dp, vertical = 10.dp),
            )
        }
    }
}

@Composable
private fun PerfScreen(state: DeskState) {
    val perf = state.perf
    Column(Modifier.fillMaxSize(), verticalArrangement = Arrangement.spacedBy(10.dp)) {
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
        val size = minOf(38f, (maxWidth.value - 28f) / 1.6f).sp
        Liquid(value, Modifier.fillMaxSize())
        Column(Modifier.padding(14.dp)) {
            Text(label, color = Palette.ink, fontSize = 14.sp)
            Percent(value, size)
        }
    }
}

/** 값만큼 아래에서 차오르는 물결 */
@Composable
private fun Liquid(value: Float?, modifier: Modifier) {
    val level by animateFloatAsState((value ?: 0f).coerceIn(0f, 100f) / 100f, tween(600), label = "level")
    val phase by rememberInfiniteTransition(label = "wave").animateFloat(
        0f,
        (2 * PI).toFloat(),
        infiniteRepeatable(tween(4_000, easing = LinearEasing)),
        label = "phase",
    )
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
        drawPath(path, Palette.liquid)
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
            withStyle(SpanStyle(fontSize = 20.sp, fontWeight = FontWeight.Medium)) { append(number) }
            withStyle(SpanStyle(fontSize = 12.sp)) { append(" $unit") }
        },
        color = Palette.ink,
        fontSize = 14.sp,
    )
}

@Composable
private fun NetGraph(received: List<Long>, sent: List<Long>, modifier: Modifier) {
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
        line(sent, Palette.muted)
        line(received, Palette.accent)
    }
}

@Composable
private fun ProcessScreen(state: DeskState, actions: DeskActions) {
    Column(Modifier.fillMaxSize()) {
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
private fun ClockTile(modifier: Modifier) {
    var now by remember { mutableLongStateOf(System.currentTimeMillis()) }
    LaunchedEffect(Unit) {
        while (true) {
            delay(1_000 - now % 1_000)
            now = System.currentTimeMillis()
        }
    }
    Box(modifier.clip(RoundedCornerShape(20.dp)).background(Palette.side), contentAlignment = Alignment.Center) {
        Text(
            SimpleDateFormat("HH:mm", Locale.KOREA).format(Date(now)),
            color = Palette.ink,
            fontSize = 26.sp,
            fontWeight = FontWeight.Medium,
        )
    }
}

@Composable
private fun CpuTile(cpu: Float?, modifier: Modifier) {
    Box(modifier.clip(RoundedCornerShape(20.dp)).background(Palette.side)) {
        Liquid(cpu, Modifier.fillMaxSize())
        Row(Modifier.fillMaxSize().padding(horizontal = 14.dp), verticalAlignment = Alignment.CenterVertically) {
            Text("CPU", color = Palette.ink, fontSize = 12.sp)
            Spacer(Modifier.weight(1f))
            Percent(cpu, 26.sp)
        }
    }
}

@Composable
private fun TopProcessTile(top: ProcessInfo?, modifier: Modifier) {
    Column(
        modifier.clip(RoundedCornerShape(20.dp)).background(Palette.side).padding(horizontal = 14.dp),
        verticalArrangement = Arrangement.Center,
    ) {
        Text(top?.name ?: "프로세스", color = Palette.ink, fontSize = 14.sp, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis)
        Text(top?.let { "%.1f%%".format(it.cpu) } ?: "-", color = Palette.muted, fontSize = 12.sp)
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
