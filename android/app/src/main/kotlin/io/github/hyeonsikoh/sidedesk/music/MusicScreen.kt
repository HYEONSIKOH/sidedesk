package io.github.hyeonsikoh.sidedesk.music

import io.github.hyeonsikoh.sidedesk.ui.MiniLabel
import io.github.hyeonsikoh.sidedesk.ui.MiniName
import io.github.hyeonsikoh.sidedesk.ui.MiniTile
import android.graphics.BitmapFactory
import io.github.hyeonsikoh.sidedesk.ui.Track
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import io.github.hyeonsikoh.sidedesk.link.NowPlaying
import io.github.hyeonsikoh.sidedesk.ui.Palette
import io.github.hyeonsikoh.sidedesk.ui.fit
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import io.github.hyeonsikoh.sidedesk.clock.rememberStarted

private const val DIM = 0.4f

class MusicActions(
    val playPause: () -> Unit,
    val next: () -> Unit,
    val previous: () -> Unit,
    val dragVolume: (Int) -> Unit,
    val releaseVolume: () -> Unit,
)

/**
 * 재생 중이고 화면이 보이는 동안만 0.5초마다 시각을 다시 읽는다. 멈췄으면 진행 바가 서 있어 읽지 않는다.
 * 화면이 다시 보이면(START) 곧바로 다시 읽는다.
 */
@Composable
fun rememberTicks(clock: () -> Long, running: Boolean): Long {
    var value by remember { mutableLongStateOf(clock()) }
    val started = rememberStarted()
    LaunchedEffect(clock, running, started) {
        value = clock()
        while (running && started) {
            delay(500)
            value = clock()
        }
    }
    return value
}

/** jpeg 를 백그라운드에서 푼다. 깨졌으면 null 이라 기본 그림이 나온다. */
@Composable
private fun rememberArt(jpeg: ByteArray?): ImageBitmap? = produceState<ImageBitmap?>(null, jpeg) {
    value = jpeg?.let {
        withContext(Dispatchers.Default) {
            runCatching { BitmapFactory.decodeByteArray(it, 0, it.size)?.asImageBitmap() }.getOrNull()
        }
    }
}.value

/** 음악 큰 화면(시안 다섯째 그림). 가로는 왼쪽 그림, 세로는 위 그림 */
@Composable
fun MusicBig(state: MusicState, nowMs: Long, actions: MusicActions, modifier: Modifier = Modifier) {
    Track("music-big")
    BoxWithConstraints(modifier.fillMaxSize()) {
        val layout = musicLayout(maxWidth.value, maxHeight.value)
        val wide = !layout.stacked
        val art = layout.art
        val body = Modifier.alpha(if (state.dimmed) DIM else 1f)
        if (wide) {
            Row(body.fillMaxSize().padding(8.dp), verticalAlignment = Alignment.CenterVertically) {
                Artwork(state, art.dp)
                Spacer(Modifier.width(24.dp))
                Info(state, nowMs, actions, Modifier.weight(1f))
            }
        } else {
            Column(body.fillMaxSize().padding(8.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.Center) {
                Artwork(state, art.dp)
                Spacer(Modifier.height(18.dp))
                Info(state, nowMs, actions, Modifier.fillMaxWidth())
            }
        }
        state.notice?.let {
            Text(
                it,
                color = Palette.ground,
                fontSize = fit(14.dp),
                modifier = Modifier
                    .align(Alignment.BottomCenter)
                    .padding(8.dp)
                    .clip(RoundedCornerShape(20.dp))
                    .background(Palette.ink)
                    .padding(horizontal = 18.dp, vertical = 10.dp),
            )
        }
    }
}

@Composable
private fun Artwork(state: MusicState, size: Dp) {
    val image = rememberArt(state.art)
    val shape = RoundedCornerShape(size * 0.1f)
    if (image != null) {
        Image(image, null, contentScale = ContentScale.Crop, modifier = Modifier.size(size).clip(shape))
    } else {
        DefaultArt(Modifier.size(size).clip(shape))
    }
}

/** 그림이 없을 때의 기본 그림. 시안의 하늘, 해, 물결 */
@Composable
fun DefaultArt(modifier: Modifier) {
    Canvas(modifier) {
        drawRect(SKY)
        drawCircle(SUN, size.minDimension * 0.15f, Offset(size.width * 0.7f, size.height * 0.3f))
        fun wave(top: Float, color: Color) {
            val path = Path().apply {
                moveTo(0f, size.height * top)
                cubicTo(size.width * 0.3f, size.height * (top - 0.1f), size.width * 0.6f, size.height * (top + 0.08f), size.width, size.height * (top - 0.04f))
                lineTo(size.width, size.height)
                lineTo(0f, size.height)
                close()
            }
            drawPath(path, color)
        }
        wave(0.6f, WAVE_LIGHT)
        wave(0.74f, WAVE)
    }
}

private val SKY = Color(0xFF7FA0B4)
private val SUN = Color(0xFFF3DFA8)
private val WAVE_LIGHT = Color(0xFF5A7E94)
private val WAVE = Color(0xFF3E6075)

@Composable
private fun Info(state: MusicState, nowMs: Long, actions: MusicActions, modifier: Modifier) {
    val now = state.now
    Column(modifier) {
        if (state.hasTrack && now != null) {
            Text(playerName(now.player), color = Palette.muted, fontSize = fit(13.dp), maxLines = 1)
            Text(now.title, color = Palette.ink, fontSize = fit(22.dp), fontWeight = FontWeight.SemiBold, maxLines = 2, overflow = TextOverflow.Ellipsis)
            if (now.artist.isNotBlank()) {
                Text(now.artist, color = Palette.muted, fontSize = fit(14.dp), maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            Spacer(Modifier.height(14.dp))
            Progress(now, nowMs, state.lostAt)
        } else {
            Text("재생 중인 곡 없음", color = Palette.muted, fontSize = fit(18.dp), maxLines = 1)
            Spacer(Modifier.height(14.dp))
        }
        Spacer(Modifier.height(10.dp))
        Controls(state, actions)
        state.volume?.takeIf { it.available }?.let {
            Spacer(Modifier.height(14.dp))
            VolumeBar(state, actions)
        }
    }
}

@Composable
private fun Progress(now: NowPlaying, nowMs: Long, lostAt: Long?) {
    val position = positionAt(now, nowMs, lostAt)
    if (now.durationMs > 0) {
        Bar(position.toFloat() / now.durationMs, Modifier.fillMaxWidth().height(6.dp))
        Row(Modifier.fillMaxWidth().padding(top = 6.dp)) {
            Text(clockText(position), color = Palette.muted, fontSize = fit(12.dp))
            Spacer(Modifier.weight(1f))
            Text(clockText(now.durationMs), color = Palette.muted, fontSize = fit(12.dp))
        }
    } else {
        // 전체 길이를 모르면 막대 없이 지난 시간만
        Text(clockText(position), color = Palette.muted, fontSize = fit(12.dp))
    }
}

@Composable
private fun Bar(fraction: Float, modifier: Modifier) {
    val fill = Palette.accent
    val track = if (Palette.dark) Color(0xFF3A424C) else Color(0xFFD5DCE4)
    Canvas(modifier.clip(CircleShape)) {
        drawRect(track)
        drawRect(fill, size = size.copy(width = size.width * fraction.coerceIn(0f, 1f)))
    }
}

@Composable
private fun Controls(state: MusicState, actions: MusicActions) {
    val now = state.now
    val live = state.hasTrack && !state.dimmed
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceEvenly, verticalAlignment = Alignment.CenterVertically) {
        SkipButton(back = true, enabled = live && now?.canPrevious == true, onClick = actions.previous)
        PlayButton(state.playing, enabled = live && now?.canPlay == true, diameter = 64.dp, onClick = actions.playPause)
        SkipButton(back = false, enabled = live && now?.canNext == true, onClick = actions.next)
    }
}

/** 재생과 멈춤 그림은 돌며 바뀐다(시안 .ppi). */
@Composable
fun PlayButton(playing: Boolean, enabled: Boolean, diameter: Dp, onClick: () -> Unit, filled: Boolean = true) {
    val turn by animateFloatAsState(if (playing) 0f else 90f, label = "pp")
    // 흐림은 층(alpha)이 아니라 색의 투명도로 한다. 층을 켰다 끄면 다시 그려지지 않는 일이 있었다.
    val fade = if (enabled) 1f else DIM
    val ink = (if (filled) Palette.panel else Palette.ink).copy(alpha = fade)
    Box(
        Modifier
            .size(maxOf(diameter, 48.dp))
            .clip(CircleShape)
            .clickable(enabled = enabled, onClickLabel = "재생 또는 멈춤", onClick = onClick)
            .semantics { contentDescription = "재생 또는 멈춤" },
        contentAlignment = Alignment.Center,
    ) {
        Box(
            Modifier.size(diameter).clip(CircleShape).background((if (filled) Palette.accent else Palette.card).copy(alpha = fade)),
            contentAlignment = Alignment.Center,
        ) {
            Canvas(Modifier.size(diameter * 0.4f).rotate(turn - if (playing) 0f else 90f)) {
                if (playing) {
                    val w = size.width * 0.28f
                    drawRect(ink, Offset(size.width * 0.12f, 0f), size.copy(width = w))
                    drawRect(ink, Offset(size.width * 0.6f, 0f), size.copy(width = w))
                } else {
                    val path = Path().apply {
                        moveTo(size.width * 0.15f, 0f)
                        lineTo(size.width, size.height / 2)
                        lineTo(size.width * 0.15f, size.height)
                        close()
                    }
                    drawPath(path, ink)
                }
            }
        }
    }
}

@Composable
private fun SkipButton(back: Boolean, enabled: Boolean, onClick: () -> Unit) {
    val ink = Palette.ink.copy(alpha = if (enabled) 1f else DIM)
    Box(
        Modifier
            .size(48.dp)
            .clip(CircleShape)
            .clickable(enabled = enabled, onClick = onClick)
            .semantics { contentDescription = if (back) "이전 곡" else "다음 곡" },
        contentAlignment = Alignment.Center,
    ) {
        Canvas(Modifier.size(22.dp).rotate(if (back) 180f else 0f)) {
            val path = Path().apply {
                moveTo(0f, 0f)
                lineTo(size.width * 0.78f, size.height / 2)
                lineTo(0f, size.height)
                close()
            }
            drawPath(path, ink)
            drawRect(ink, Offset(size.width * 0.82f, 0f), size.copy(width = size.width * 0.16f))
        }
    }
}

@Composable
private fun VolumeBar(state: MusicState, actions: MusicActions) {
    val volume = state.volume ?: return
    val usable = volume.settable && !state.dimmed
    val ink = Palette.ink
    Row(verticalAlignment = Alignment.CenterVertically) {
        Canvas(Modifier.size(20.dp)) {
            val line = Stroke(size.minDimension * 0.09f, cap = StrokeCap.Round, join = StrokeJoin.Round)
            val body = Path().apply {
                moveTo(size.width * 0.1f, size.height * 0.38f)
                lineTo(size.width * 0.3f, size.height * 0.38f)
                lineTo(size.width * 0.55f, size.height * 0.15f)
                lineTo(size.width * 0.55f, size.height * 0.85f)
                lineTo(size.width * 0.3f, size.height * 0.62f)
                lineTo(size.width * 0.1f, size.height * 0.62f)
                close()
            }
            drawPath(body, ink, style = line)
            if (volume.muted) {
                drawLine(ink, Offset(size.width * 0.68f, size.height * 0.35f), Offset(size.width * 0.95f, size.height * 0.65f), line.width, StrokeCap.Round)
                drawLine(ink, Offset(size.width * 0.95f, size.height * 0.35f), Offset(size.width * 0.68f, size.height * 0.65f), line.width, StrokeCap.Round)
            } else {
                drawArc(ink, -45f, 90f, false, Offset(size.width * 0.5f, size.height * 0.3f), size.copy(width = size.width * 0.3f, height = size.height * 0.4f), style = line)
            }
        }
        Spacer(Modifier.width(10.dp))
        val fraction = (state.level ?: 0) / 100f
        BoxWithConstraints(
            Modifier
                .weight(1f)
                .height(28.dp)
                .alpha(if (usable && !volume.muted) 1f else DIM)
                .then(
                    if (usable) {
                        Modifier
                            .pointerInput(Unit) {
                                detectTapGestures { actions.dragVolume((it.x / size.width * 100).toInt()); actions.releaseVolume() }
                            }
                            .pointerInput(Unit) {
                                detectDragGestures(
                                    onDragEnd = actions.releaseVolume,
                                    onDragCancel = actions.releaseVolume,
                                ) { change, _ -> actions.dragVolume((change.position.x / size.width * 100).toInt()) }
                            }
                    } else {
                        Modifier
                    },
                ),
            contentAlignment = Alignment.Center,
        ) {
            Bar(fraction, Modifier.fillMaxWidth().height(6.dp))
        }
    }
}

/**
 * 음악 작은 칸(시안 .mn, .edge). 왼쪽 버튼은 재생/멈춤이고, 칸의 나머지는 음악 화면으로 간다.
 * 칸 아래 끝에 얇은 진행 줄.
 */
@Composable
fun MusicTile(state: MusicState, nowMs: Long, actions: MusicActions, modifier: Modifier) {
    Track("music-tile")
    val now = state.now
    if (!state.hasTrack || now == null) {
        MiniTile(modifier, first = { size -> MiniName("음악", size) }, second = { MiniLabel("재생 중인 곡 없음") })
        return
    }
    Box(modifier.clip(RoundedCornerShape(20.dp)).background(Palette.side)) {
        MiniTile(
            Modifier.fillMaxSize().alpha(if (state.dimmed) DIM else 1f),
            background = Palette.side,
            first = { size ->
                PlayButton(state.playing, enabled = !state.dimmed && now.canPlay, diameter = if (size.low) 30.dp else 36.dp, onClick = actions.playPause, filled = false)
            },
            second = { size -> MiniName(now.title, size, Modifier.padding(bottom = if (size.wide) 0.dp else 6.dp)) },
        )
        if (now.durationMs > 0) {
            // 칸 아래 끝 얇은 진행 줄(왼쪽, 오른쪽 14, 아래 7, 굵기 4)
            Bar(
                positionAt(now, nowMs, state.lostAt).toFloat() / now.durationMs,
                Modifier.align(Alignment.BottomCenter).fillMaxWidth().padding(start = 14.dp, end = 14.dp, bottom = 7.dp).height(4.dp),
            )
        }
    }
}

/** 음악 큰 화면 배치. stacked 면 그림 위, 글 아래. art 는 그림 한 변. */
internal data class MusicLayout(val stacked: Boolean, val art: Float)

/**
 * 세로이거나, 높이 480 이상에 폭이 높이의 1.25배 이하(정사각에 가까운 큰 판)면 그림을 위에 쌓는다.
 * 쌓으면 그림은 높이 52%, 폭 78% 중 작은 쪽. 옆으로 놓으면 폭 42%(높이 340 이하로 낮으면 34%), 높이를 넘지 않게.
 */
internal fun musicLayout(width: Float, height: Float): MusicLayout {
    val stacked = width <= height || (height >= 480f && width <= height * 1.25f)
    if (stacked) return MusicLayout(true, minOf(height * 0.52f, width * 0.78f))
    val share = if (height <= 340f) 0.34f else 0.42f
    return MusicLayout(false, minOf(height - 16f, width * share))
}
