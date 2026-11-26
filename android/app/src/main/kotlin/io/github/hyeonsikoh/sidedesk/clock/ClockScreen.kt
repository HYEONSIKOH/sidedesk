package io.github.hyeonsikoh.sidedesk.clock

import androidx.compose.foundation.Canvas
import io.github.hyeonsikoh.sidedesk.ui.Track
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.LayoutCoordinates
import androidx.compose.ui.geometry.Rect
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withLink
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.hyeonsikoh.sidedesk.link.WeatherSource
import io.github.hyeonsikoh.sidedesk.ui.Palette
import io.github.hyeonsikoh.sidedesk.ui.Theme
import io.github.hyeonsikoh.sidedesk.ui.fit
import io.github.hyeonsikoh.sidedesk.rest.RestPanel
import io.github.hyeonsikoh.sidedesk.rest.RestSettings
import androidx.compose.foundation.layout.heightIn
import androidx.compose.runtime.mutableStateOf
import kotlinx.coroutines.delay
import java.util.Calendar
import java.util.Locale
import kotlin.math.PI
import kotlin.math.cos
import kotlin.math.min
import kotlin.math.sin

private const val DIM = 0.4f
private val DAYS = listOf("일", "월", "화", "수", "목", "금", "토")

/**
 * 분이 바뀔 때마다 시각을 다시 읽는다. 시계는 시와 분만 보이고 초침이 없어 1분이면 된다.
 * AI 지난 시간도 이 시각을 써서 "지금"에서 "1분"으로 넘어가는 때가 최대 59초 늦을 수 있다.
 */
@Composable
fun rememberNow(now: () -> Long): Long {
    var value by remember { mutableLongStateOf(now()) }
    LaunchedEffect(now) {
        while (true) {
            delay(60_000 - value % 60_000)
            value = now()
        }
    }
    return value
}

private fun calendar(millis: Long) = Calendar.getInstance(Locale.KOREA).apply { timeInMillis = millis }

fun timeText(millis: Long): String = calendar(millis).let { "%02d:%02d".format(it.get(Calendar.HOUR_OF_DAY), it.get(Calendar.MINUTE)) }

private fun dateText(millis: Long) = calendar(millis).let {
    "${it.get(Calendar.MONTH) + 1}월 ${it.get(Calendar.DAY_OF_MONTH)}일 ${DAYS[it.get(Calendar.DAY_OF_WEEK) - 1]}요일"
}

private fun degrees(value: Float) = "${Math.round(value)}°"

/** 동그라미가 기준으로 삼을 묶음(숫자 시각, 큰 기온, 큰 날짜 등)을 알려 주는 곳 */
private val LocalSunAnchor = staticCompositionLocalOf<(LayoutCoordinates) -> Unit> { {} }

/** 이 묶음 뒤에 시각 따라 움직이는 동그라미를 둔다. 모양마다 하나만 붙인다. */
@Composable
private fun Modifier.sunAnchor(): Modifier {
    val report = LocalSunAnchor.current
    return onGloballyPositioned(report)
}

/** 홈 큰 화면. 고른 모양 하나와, 그 모양의 기준 묶음 뒤에서 시각 따라 움직이는 동그라미 */
@Composable
fun HomeBig(clock: ClockState, now: Long, modifier: Modifier = Modifier, resting: Boolean = false) {
    Track("home-big")
    var frame by remember { mutableStateOf<LayoutCoordinates?>(null) }
    var anchor by remember(clock.face) { mutableStateOf<Rect?>(null) }
    BoxWithConstraints(modifier.fillMaxSize().onGloballyPositioned { frame = it }) {
        val w = maxWidth
        val h = maxHeight
        // 쉬는 화면에서는 해 동그라미를 그리지 않는다(검은 화면 위 밝은 원은 번인과 눈부심을 부른다).
        if (!resting) anchor?.let { SunCircle(now, it, Modifier.fillMaxSize()) }
        val report: (LayoutCoordinates) -> Unit = { coords ->
            val box = frame
            if (box != null && box.isAttached && coords.isAttached) {
                val rect = box.localBoundingBoxOf(coords, clipBounds = false)
                if (rect != anchor) anchor = rect
            }
        }
        CompositionLocalProvider(LocalSunAnchor provides report) {
            when (clock.face) {
                Face.Digits -> DigitsFace(clock, now, w, h)
                Face.Dial -> DialFace(clock, now, w, h)
                Face.Weather -> WeatherFace(clock, now, w, h)
                Face.Calendar -> CalendarFace(now, w, h)
                Face.Overview -> OverviewFace(clock, now)
            }
        }
    }
}

/**
 * 기준 묶음(anchor) 안에서 움직이는 동그라미. 시안 목업과 같은 식이다.
 * 낮(6시~19시)과 밤(19시~6시)을 각각 왼쪽에서 오른쪽으로 반원을 그리며 지나고, 크기는 묶음 높이에 맞춘다.
 */
@Composable
private fun SunCircle(now: Long, anchor: Rect, modifier: Modifier) {
    val color = Palette.card
    val c = calendar(now)
    val hour = c.get(Calendar.HOUR_OF_DAY) + c.get(Calendar.MINUTE) / 60f
    val u = if (hour >= 6 && hour < 19) (hour - 6) / 13f else ((hour - 19 + 24) % 24) / 11f
    Canvas(modifier) {
        val r = anchor.height * 0.62f
        val x = anchor.left + anchor.width * (0.08f + 0.84f * u)
        val y = anchor.top + anchor.height * (0.86f - 0.6f * sin(PI * u).toFloat())
        drawCircle(color, r, Offset(x, y))
    }
}

@Composable
private fun DateLine(clock: ClockState, now: Long, size: TextUnit) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Text(dateText(now), color = Palette.ink, fontSize = size, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
        clock.via?.let { ViaIcon(it, Palette.muted, Modifier.padding(start = 8.dp).size(with(LocalDensity.current) { size.toDp() })) }
    }
}

/**
 * 시계 숫자. 시안의 둥근 숫자에 맞추려고 가는 글꼴 위에 끝이 둥근 테두리를 같은 색으로 한 번 더 그린다.
 * 그러면 획 끝과 모서리가 둥글어진다. 색은 위에서 아래로 옅어진다.
 */
@Composable
private fun Digits(text: String, size: TextUnit) {
    val brush = Brush.verticalGradient(listOf(Palette.digitTop, Palette.digitBottom))
    val stroke = with(LocalDensity.current) { size.toPx() * 0.05f }
    val base = TextStyle(brush = brush, fontSize = size, fontWeight = FontWeight.Thin, letterSpacing = (0.04).em(size))
    Box {
        Text(text, style = base, maxLines = 1, softWrap = false)
        Text(
            text,
            style = base.copy(drawStyle = Stroke(stroke, join = StrokeJoin.Round, cap = StrokeCap.Round)),
            maxLines = 1,
            softWrap = false,
            // 겹쳐 그린 테두리라 화면 읽기에는 한 번만 잡히게 뺀다.
            modifier = Modifier.clearAndSetSemantics {},
        )
    }
}

private fun Double.em(size: TextUnit) = (size.value * this).sp

/** 하늘 아이콘, 기온, 최고와 최저. 받은 적 없으면 --° */
@Composable
private fun WeatherLine(clock: ClockState, size: TextUnit) {
    val weather = clock.weather
    Row(Modifier.alpha(if (clock.weatherDimmed) DIM else 1f), verticalAlignment = Alignment.CenterVertically) {
        if (weather != null) {
            SkyIcon(weather.sky, weather.isDay, Palette.ink, Modifier.size(with(LocalDensity.current) { (size * 1.3f).toDp() }).padding(end = 6.dp))
        }
        Text(weather?.let { degrees(it.temperature) } ?: "--°", color = Palette.ink, fontSize = size * 1.15f, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis)
        if (weather != null) {
            Text(
                "  최고 ${degrees(weather.high)} 최저 ${degrees(weather.low)}",
                color = Palette.muted,
                fontSize = size * 0.9f,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

/** 날씨 출처. Open-Meteo 는 누르면 누리집을 연다. */
@Composable
fun Credit(clock: ClockState, size: TextUnit = fit(11.dp), modifier: Modifier = Modifier) {
    val credit = clock.credit ?: return
    val text = buildAnnotatedString {
        if (clock.weather?.source == WeatherSource.OpenMeteo) {
            withLink(LinkAnnotation.Url("https://open-meteo.com/")) { append(credit) }
        } else {
            append(credit)
        }
    }
    Text(
        text,
        color = Palette.muted,
        fontSize = size,
        // 출처 글자는 잘리면 안 되므로 줄임표 대신 두 줄까지 쓴다.
        maxLines = 2,
        textAlign = TextAlign.Center,
        modifier = modifier.alpha(if (clock.weatherDimmed) DIM else 1f),
    )
}

@Composable
private fun DigitsFace(clock: ClockState, now: Long, w: Dp, h: Dp) {
    val digit = fit(min(w.value * 0.27f, h.value * 0.4f).dp)
    val line = fit(min(w.value * 0.045f, h.value * 0.06f).coerceIn(13f, 26f).dp)
    Column(Modifier.fillMaxSize().padding(16.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.Center) {
        DateLine(clock, now, line)
        Box(Modifier.sunAnchor()) { Digits(timeText(now), digit) }
        WeatherLine(clock, line)
        Credit(clock, modifier = Modifier.padding(top = 6.dp))
    }
}

@Composable
private fun DialFace(clock: ClockState, now: Long, w: Dp, h: Dp) {
    val wide = w > h
    val dial = min(if (wide) w.value * 0.45f else w.value * 0.7f, h.value * (if (wide) 0.85f else 0.5f)).dp
    val line = fit(min(w.value * 0.045f, h.value * 0.06f).coerceIn(13f, 24f).dp)
    val content = @Composable {
        Dial(now, Modifier.size(dial))
        Column(
            Modifier.padding(if (wide) PaddingStart else PaddingTop),
            horizontalAlignment = if (wide) Alignment.Start else Alignment.CenterHorizontally,
        ) {
            DateLine(clock, now, line)
            val weather = clock.weather
            Column(Modifier.alpha(if (clock.weatherDimmed) DIM else 1f), horizontalAlignment = if (wide) Alignment.Start else Alignment.CenterHorizontally) {
                Text(weather?.let { degrees(it.temperature) } ?: "--°", color = Palette.ink, fontSize = line * 2.4f, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.sunAnchor())
                if (weather != null) {
                    Text(skyName(weather.sky), color = Palette.muted, fontSize = line, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    Text("최고 ${degrees(weather.high)} 최저 ${degrees(weather.low)}", color = Palette.muted, fontSize = line, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
            Credit(clock, modifier = Modifier.padding(top = 6.dp))
        }
    }
    if (wide) {
        Row(Modifier.fillMaxSize().padding(16.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.Center) { content() }
    } else {
        Column(Modifier.fillMaxSize().padding(16.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.Center) { content() }
    }
}

private val PaddingStart = androidx.compose.foundation.layout.PaddingValues(start = 28.dp)
private val PaddingTop = androidx.compose.foundation.layout.PaddingValues(top = 20.dp)

/** 바늘 시계. 60 눈금과 12, 3, 6, 9 */
@Composable
fun Dial(now: Long, modifier: Modifier) {
    val ink = Palette.ink
    val muted = Palette.muted
    val accent = Palette.accent
    val c = calendar(now)
    val minute = c.get(Calendar.MINUTE) + c.get(Calendar.SECOND) / 60f
    val hour = c.get(Calendar.HOUR) + minute / 60f
    Box(modifier, contentAlignment = Alignment.Center) {
        Canvas(Modifier.fillMaxSize()) {
            val r = size.minDimension / 2
            val center = Offset(size.width / 2, size.height / 2)
            for (i in 0 until 60) {
                val a = i * PI / 30
                val long = i % 5 == 0
                val from = r * (if (long) 0.84f else 0.89f)
                val to = r * 0.95f
                drawLine(
                    if (long) ink else muted,
                    Offset(center.x + (sin(a) * from).toFloat(), center.y - (cos(a) * from).toFloat()),
                    Offset(center.x + (sin(a) * to).toFloat(), center.y - (cos(a) * to).toFloat()),
                    r * (if (long) 0.025f else 0.012f),
                    StrokeCap.Round,
                )
            }
            fun hand(turn: Float, length: Float, width: Float, color: androidx.compose.ui.graphics.Color) {
                val a = turn * 2 * PI
                drawLine(
                    color,
                    Offset(center.x - (sin(a) * r * 0.08f).toFloat(), center.y + (cos(a) * r * 0.08f).toFloat()),
                    Offset(center.x + (sin(a) * r * length).toFloat(), center.y - (cos(a) * r * length).toFloat()),
                    r * width,
                    StrokeCap.Round,
                )
            }
            hand(hour / 12f, 0.5f, 0.06f, ink)
            hand(minute / 60f, 0.78f, 0.04f, accent)
            drawCircle(ink, r * 0.05f, center)
        }
        BoxWithConstraints(Modifier.fillMaxSize()) {
            val r = maxWidth.value / 2
            val label = fit((r * 0.16f).dp)
            listOf(12, 3, 6, 9).forEach { n ->
                val a = n * PI / 6
                Text(
                    "$n",
                    color = ink,
                    fontSize = label,
                    textAlign = TextAlign.Center,
                    modifier = Modifier
                        .align(Alignment.Center)
                        .padding(
                            start = (if (sin(a) > 0.1) r * 1.36f else 0f).dp,
                            end = (if (sin(a) < -0.1) r * 1.36f else 0f).dp,
                            top = (if (cos(a) < -0.1) r * 1.36f else 0f).dp,
                            bottom = (if (cos(a) > 0.1) r * 1.36f else 0f).dp,
                        ),
                )
            }
        }
    }
}

@Composable
private fun WeatherFace(clock: ClockState, now: Long, w: Dp, h: Dp) {
    val big = fit(min(w.value * 0.2f, h.value * 0.3f).dp)
    val line = fit(min(w.value * 0.045f, h.value * 0.06f).coerceIn(13f, 24f).dp)
    val weather = clock.weather
    Column(Modifier.fillMaxSize().padding(16.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.Center) {
        Row(Modifier.alpha(if (clock.weatherDimmed) DIM else 1f), verticalAlignment = Alignment.CenterVertically) {
            Text(weather?.let { degrees(it.temperature) } ?: "--°", color = Palette.ink, fontSize = big, fontWeight = FontWeight.Light, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.sunAnchor())
            if (weather != null) {
                Column(Modifier.padding(start = 16.dp)) {
                    SkyIcon(weather.sky, weather.isDay, Palette.ink, Modifier.size(with(LocalDensity.current) { (line * 2.6f).toDp() }))
                    Text(skyName(weather.sky), color = Palette.ink, fontSize = line, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    Text("최고 ${degrees(weather.high)} 최저 ${degrees(weather.low)}", color = Palette.muted, fontSize = line * 0.9f, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
        }
        Credit(clock, modifier = Modifier.padding(top = 4.dp))
        Spacer(Modifier.height(12.dp))
        DateLine(clock, now, line)
        Digits(timeText(now), line * 3f)
    }
}

@Composable
private fun CalendarFace(now: Long, w: Dp, h: Dp) {
    val c = calendar(now)
    val wide = w > h
    val line = fit(min(w.value * 0.04f, h.value * 0.055f).coerceIn(12f, 22f).dp)
    val left = @Composable {
        Column(horizontalAlignment = if (wide) Alignment.Start else Alignment.CenterHorizontally) {
            Text("${c.get(Calendar.MONTH) + 1}월 ${DAYS[c.get(Calendar.DAY_OF_WEEK) - 1]}요일", color = Palette.muted, fontSize = line, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text("${c.get(Calendar.DAY_OF_MONTH)}", color = Palette.ink, fontSize = line * 5f, fontWeight = FontWeight.Light, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.sunAnchor())
            Digits(timeText(now), line * 2.2f)
        }
    }
    if (wide) {
        Row(Modifier.fillMaxSize().padding(20.dp), verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.weight(0.4f)) { left() }
            Month(now, line, Modifier.weight(0.6f))
        }
    } else {
        Column(Modifier.fillMaxSize().padding(20.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.Center) {
            left()
            Spacer(Modifier.height(16.dp))
            Month(now, line, Modifier.fillMaxWidth())
        }
    }
}

/** 이번 달 달력. 오늘은 동그라미 */
@Composable
private fun Month(now: Long, size: TextUnit, modifier: Modifier) {
    val c = calendar(now)
    val today = c.get(Calendar.DAY_OF_MONTH)
    val days = c.getActualMaximum(Calendar.DAY_OF_MONTH)
    val first = (c.clone() as Calendar).apply { set(Calendar.DAY_OF_MONTH, 1) }.get(Calendar.DAY_OF_WEEK) - 1
    val cells = List(first) { 0 } + (1..days).toList()
    Column(modifier) {
        Row(Modifier.fillMaxWidth()) {
            DAYS.forEach { Text(it, color = Palette.muted, fontSize = size * 0.85f, textAlign = TextAlign.Center, modifier = Modifier.weight(1f)) }
        }
        cells.chunked(7).forEach { week ->
            Row(Modifier.fillMaxWidth().padding(top = 4.dp)) {
                (week + List(7 - week.size) { 0 }).forEach { day ->
                    Box(Modifier.weight(1f), contentAlignment = Alignment.Center) {
                        if (day > 0) {
                            val on = day == today
                            Text(
                                "$day",
                                color = if (on) Palette.panel else Palette.ink,
                                fontSize = size * 0.9f,
                                textAlign = TextAlign.Center,
                                modifier = Modifier
                                    .clip(CircleShape)
                                    .background(if (on) Palette.accent else androidx.compose.ui.graphics.Color.Transparent)
                                    .padding(horizontal = 6.dp, vertical = 2.dp),
                            )
                        }
                    }
                }
            }
        }
    }
}

/**
 * 모아 보기: 홈의 것만 칸 넷(바늘, 날씨, 날짜, 숫자 시각)으로 모은다.
 * 성능과 프로세스는 옆의 작은 칸에 늘 있으므로 넣지 않는다(같은 값을 한 화면에 두 번 두지 않음).
 */
@Composable
private fun OverviewFace(clock: ClockState, now: Long) {
    BoxWithConstraints(Modifier.fillMaxSize().padding(4.dp)) {
        val gap = 10.dp
        val (cols, rows) = overviewGrid(maxWidth.value, maxHeight.value, gap.value)
        val cellW = (maxWidth.value - gap.value * (cols - 1)) / cols
        val cellH = (maxHeight.value - gap.value * (rows - 1)) / rows
        val line = fit(min(cellW * 0.08f, cellH * 0.1f).coerceIn(11f, 20f).dp)
        val cell = Modifier.clip(RoundedCornerShape(22.dp)).background(Palette.card)
        val c = calendar(now)
        val cells: List<@Composable (Modifier) -> Unit> = listOf(
            { m -> Box(m.then(cell).padding(12.dp), contentAlignment = Alignment.Center) { Dial(now, Modifier.fillMaxSize().aspectRatio(1f, matchHeightConstraintsFirst = cellH < cellW)) } },
            { m ->
                Column(m.then(cell).padding(14.dp), verticalArrangement = Arrangement.Center, horizontalAlignment = Alignment.CenterHorizontally) {
                    val weather = clock.weather
                    Column(Modifier.alpha(if (clock.weatherDimmed) DIM else 1f), horizontalAlignment = Alignment.CenterHorizontally) {
                        if (weather != null) SkyIcon(weather.sky, weather.isDay, Palette.ink, Modifier.size(with(LocalDensity.current) { (line * 2f).toDp() }))
                        Text(weather?.let { degrees(it.temperature) } ?: "--°", color = Palette.ink, fontSize = line * 2.4f, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        if (weather != null) Text("최고 ${degrees(weather.high)} 최저 ${degrees(weather.low)}", color = Palette.muted, fontSize = line * 0.9f, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                    Credit(clock, size = line * 0.7f, modifier = Modifier.padding(top = 4.dp))
                }
            },
            { m ->
                Column(m.then(cell).padding(14.dp), verticalArrangement = Arrangement.Center, horizontalAlignment = Alignment.CenterHorizontally) {
                    Text("${c.get(Calendar.MONTH) + 1}월", color = Palette.muted, fontSize = line, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    Text("${c.get(Calendar.DAY_OF_MONTH)}", color = Palette.ink, fontSize = line * 3.4f, fontWeight = FontWeight.Light, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    Text("${DAYS[c.get(Calendar.DAY_OF_WEEK) - 1]}요일", color = Palette.ink, fontSize = line, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            },
            { m ->
                Column(m.then(cell).padding(14.dp), verticalArrangement = Arrangement.Center, horizontalAlignment = Alignment.CenterHorizontally) {
                    Digits(timeText(now), fit(min(cellW * 0.22f, cellH * 0.35f).dp))
                    clock.via?.let { ViaIcon(it, Palette.muted, Modifier.padding(top = 6.dp).size(with(LocalDensity.current) { line.toDp() })) }
                }
            },
        )
        Column(Modifier.fillMaxSize(), verticalArrangement = Arrangement.spacedBy(gap)) {
            cells.chunked(cols).forEach { row ->
                Row(Modifier.weight(1f).fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(gap)) {
                    row.forEach { it(Modifier.weight(1f).fillMaxHeight()) }
                }
            }
        }
    }
}

/** 칸 넷을 4x1, 2x2, 1x4 가운데 칸이 가장 덜 길쭉해지는 배치로 놓는다. */
internal fun overviewGrid(width: Float, height: Float, gap: Float): Pair<Int, Int> =
    listOf(4 to 1, 2 to 2, 1 to 4).minBy { (cols, rows) ->
        val w = (width - gap * (cols - 1)) / cols
        val h = (height - gap * (rows - 1)) / rows
        kotlin.math.abs(kotlin.math.ln(w / h))
    }

/** 다른 화면을 볼 때의 홈 작은 칸. 시각, 하늘 아이콘, 기온, 받은 곳 */
@Composable
fun HomeTile(clock: ClockState, now: Long, modifier: Modifier) {
    Track("home-tile")
    BoxWithConstraints(modifier.clip(RoundedCornerShape(20.dp)).background(Palette.side).padding(horizontal = 8.dp, vertical = 6.dp)) {
        // 좁은 세로에서 칸이 셋이면 칸이 100dp 남짓이라, 시각과 기온이 함께 들어가게 너비에 맞춘다.
        val timeSize = fit(min(24f, maxWidth.value * 0.2f).dp)
        Row(Modifier.align(Alignment.Center), verticalAlignment = Alignment.CenterVertically) {
            Text(timeText(now), color = Palette.ink, fontSize = timeSize, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis)
            val weather = clock.weather
            Column(Modifier.padding(start = 6.dp).alpha(if (clock.weatherDimmed) DIM else 1f), horizontalAlignment = Alignment.CenterHorizontally) {
                if (weather != null) SkyIcon(weather.sky, weather.isDay, Palette.ink, Modifier.size(14.dp))
                Text(weather?.let { degrees(it.temperature) } ?: "--°", color = Palette.ink, fontSize = fit(11.dp), maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
        clock.shortCredit?.let {
            Text(
                it,
                color = Palette.muted,
                fontSize = fit(7.dp),
                maxLines = 1,
                modifier = Modifier.align(Alignment.BottomCenter).alpha(if (clock.weatherDimmed) DIM else 1f),
            )
        }
    }
}

class PickActions(
    val setFace: (Face) -> Unit,
    val setTheme: (Theme) -> Unit,
    val done: () -> Unit,
)

/** 길게 누르면 뜨는 고르기 줄. 모양 다섯, 테마 둘, 쉬기 설정, 완료 */
@Composable
fun Picker(clock: ClockState, actions: PickActions, modifier: Modifier, rest: RestPanel? = null, restOpen: Boolean = false) {
    var resting by remember { mutableStateOf(restOpen) }
    Column(
        modifier
            .clip(RoundedCornerShape(22.dp))
            .background(Palette.raised)
            .padding(10.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        if (resting && rest != null) {
            RestSettings(rest, Modifier.heightIn(max = 180.dp))
        } else {
            Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                Face.entries.forEach { face -> Chip(face.label, face == clock.face) { actions.setFace(face) } }
            }
        }
        Row(verticalAlignment = Alignment.CenterVertically) {
            if (!resting) {
                Chip("라이트", clock.theme == Theme.Light) { actions.setTheme(Theme.Light) }
                Spacer(Modifier.width(6.dp))
                Chip("다크", clock.theme == Theme.Dark) { actions.setTheme(Theme.Dark) }
                Spacer(Modifier.width(6.dp))
            }
            if (rest != null) Chip(if (resting) "모양" else "쉬기", false) { resting = !resting }
            Spacer(Modifier.weight(1f))
            Text(
                "완료",
                color = Palette.panel,
                fontSize = fit(14.dp),
                maxLines = 1,
                fontWeight = FontWeight.SemiBold,
                modifier = Modifier
                    .clip(CircleShape)
                    .background(Palette.accent)
                    .clickable(onClick = actions.done)
                    .padding(horizontal = 20.dp, vertical = 9.dp),
            )
        }
    }
}

@Composable
private fun Chip(label: String, on: Boolean, onClick: () -> Unit) {
    Text(
        label,
        color = if (on) Palette.panel else Palette.ink,
        fontSize = fit(13.dp),
        fontWeight = if (on) FontWeight.SemiBold else FontWeight.Normal,
        maxLines = 1,
        modifier = Modifier
            .clip(CircleShape)
            .background(if (on) Palette.accent else Palette.card)
            .clickable(onClick = onClick)
            .padding(horizontal = 14.dp, vertical = 8.dp),
    )
}
