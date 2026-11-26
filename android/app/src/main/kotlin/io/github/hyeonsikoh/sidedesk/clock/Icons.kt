package io.github.hyeonsikoh.sidedesk.clock

import androidx.compose.foundation.Canvas
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import io.github.hyeonsikoh.sidedesk.link.Sky
import io.github.hyeonsikoh.sidedesk.link.Via
import kotlin.math.PI
import kotlin.math.cos
import kotlin.math.sin

fun skyName(sky: Sky) = when (sky) {
    Sky.Unknown -> ""
    Sky.Clear -> "맑음"
    Sky.PartlyCloudy -> "구름 조금"
    Sky.Cloudy -> "흐림"
    Sky.Fog -> "안개"
    Sky.Drizzle -> "이슬비"
    Sky.Rain -> "비"
    Sky.Snow -> "눈"
    Sky.Shower -> "소나기"
    Sky.Thunderstorm -> "뇌우"
}

/** 하늘 아이콘. 선으로만 그린다. 모르면 그리지 않는다. */
@Composable
fun SkyIcon(sky: Sky, isDay: Boolean, color: Color, modifier: Modifier) {
    if (sky == Sky.Unknown) return
    Canvas(modifier) {
        val line = Stroke(size.minDimension * 0.075f, cap = StrokeCap.Round, join = StrokeJoin.Round)
        val night = !isDay && (sky == Sky.Clear || sky == Sky.PartlyCloudy)
        when (sky) {
            Sky.Clear -> if (night) moon(color, line, 0.5f, 0.5f, 0.3f) else sun(color, line, 0.5f, 0.5f, 0.18f)
            Sky.PartlyCloudy -> {
                if (night) moon(color, line, 0.36f, 0.34f, 0.2f) else sun(color, line, 0.36f, 0.34f, 0.12f)
                cloud(color, line, 0.58f, 0.66f, 0.62f)
            }
            Sky.Cloudy -> cloud(color, line, 0.5f, 0.56f, 0.86f)
            Sky.Fog -> for (i in 0..2) {
                val y = size.height * (0.34f + i * 0.16f)
                drawLine(color, Offset(size.width * 0.18f, y), Offset(size.width * 0.82f, y), line.width, StrokeCap.Round)
            }
            else -> {
                cloud(color, line, 0.5f, 0.42f, 0.8f)
                val y0 = size.height * 0.7f
                val y1 = size.height * 0.86f
                when (sky) {
                    Sky.Drizzle -> for (x in listOf(0.34f, 0.5f, 0.66f)) {
                        drawCircle(color, line.width * 0.7f, Offset(size.width * x, (y0 + y1) / 2))
                    }
                    Sky.Snow -> for (x in listOf(0.34f, 0.5f, 0.66f)) {
                        snowflake(color, line.width * 0.6f, Offset(size.width * x, (y0 + y1) / 2), size.minDimension * 0.06f)
                    }
                    Sky.Thunderstorm -> {
                        val bolt = Path().apply {
                            moveTo(size.width * 0.54f, y0 - size.height * 0.04f)
                            lineTo(size.width * 0.44f, (y0 + y1) / 2)
                            lineTo(size.width * 0.56f, (y0 + y1) / 2)
                            lineTo(size.width * 0.46f, y1 + size.height * 0.04f)
                        }
                        drawPath(bolt, color, style = line)
                    }
                    else -> {
                        val slant = if (sky == Sky.Shower) size.width * 0.06f else 0f
                        for (x in listOf(0.34f, 0.5f, 0.66f)) {
                            drawLine(color, Offset(size.width * x + slant, y0), Offset(size.width * x - slant, y1), line.width, StrokeCap.Round)
                        }
                    }
                }
            }
        }
    }
}

/** 날짜 옆 연결 아이콘 */
@Composable
fun ViaIcon(via: Via, color: Color, modifier: Modifier) {
    Canvas(modifier) {
        val line = Stroke(size.minDimension * 0.09f, cap = StrokeCap.Round, join = StrokeJoin.Round)
        val w = size.width
        val h = size.height
        when (via) {
            Via.Usb -> {
                drawLine(color, Offset(w * 0.5f, h * 0.12f), Offset(w * 0.5f, h * 0.82f), line.width, StrokeCap.Round)
                drawCircle(color, w * 0.08f, Offset(w * 0.5f, h * 0.86f))
                val left = Path().apply {
                    moveTo(w * 0.5f, h * 0.62f)
                    lineTo(w * 0.28f, h * 0.48f)
                    lineTo(w * 0.28f, h * 0.34f)
                }
                drawPath(left, color, style = line)
                val right = Path().apply {
                    moveTo(w * 0.5f, h * 0.52f)
                    lineTo(w * 0.72f, h * 0.4f)
                    lineTo(w * 0.72f, h * 0.28f)
                }
                drawPath(right, color, style = line)
                val head = Path().apply {
                    moveTo(w * 0.4f, h * 0.2f)
                    lineTo(w * 0.5f, h * 0.06f)
                    lineTo(w * 0.6f, h * 0.2f)
                }
                drawPath(head, color, style = line)
            }
            Via.Bluetooth -> {
                val rune = Path().apply {
                    moveTo(w * 0.28f, h * 0.3f)
                    lineTo(w * 0.7f, h * 0.68f)
                    lineTo(w * 0.5f, h * 0.86f)
                    lineTo(w * 0.5f, h * 0.14f)
                    lineTo(w * 0.7f, h * 0.32f)
                    lineTo(w * 0.28f, h * 0.7f)
                }
                drawPath(rune, color, style = line)
            }
        }
    }
}

private fun DrawScope.sun(color: Color, line: Stroke, cx: Float, cy: Float, r: Float) {
    val center = Offset(size.width * cx, size.height * cy)
    val radius = size.minDimension * r
    drawCircle(color, radius, center, style = line)
    for (i in 0 until 8) {
        val a = i * PI / 4
        val from = radius * 1.5f
        val to = radius * 2.0f
        drawLine(
            color,
            Offset(center.x + (cos(a) * from).toFloat(), center.y + (sin(a) * from).toFloat()),
            Offset(center.x + (cos(a) * to).toFloat(), center.y + (sin(a) * to).toFloat()),
            line.width,
            StrokeCap.Round,
        )
    }
}

private fun DrawScope.moon(color: Color, line: Stroke, cx: Float, cy: Float, r: Float) {
    val radius = size.minDimension * r
    val center = Offset(size.width * cx, size.height * cy)
    drawPath(io.github.hyeonsikoh.sidedesk.shortcut.crescent(center, radius), color, style = line)
}

private fun DrawScope.cloud(color: Color, line: Stroke, cx: Float, cy: Float, width: Float) {
    val w = size.width * width
    val h = w * 0.5f
    val left = size.width * cx - w / 2
    val top = size.height * cy - h / 2
    val path = Path().apply {
        moveTo(left + h * 0.5f, top + h)
        arcTo(androidx.compose.ui.geometry.Rect(Offset(left, top + h * 0.3f), Size(h * 0.7f, h * 0.7f)), 90f, 180f, false)
        arcTo(androidx.compose.ui.geometry.Rect(Offset(left + w * 0.2f, top - h * 0.1f), Size(w * 0.45f, w * 0.45f)), 200f, 150f, false)
        arcTo(androidx.compose.ui.geometry.Rect(Offset(left + w * 0.58f, top + h * 0.25f), Size(h * 0.75f, h * 0.75f)), 280f, 170f, false)
        close()
    }
    drawPath(path, color, style = line)
}

private fun DrawScope.snowflake(color: Color, width: Float, center: Offset, r: Float) {
    for (i in 0 until 3) {
        val a = i * PI / 3
        val dx = (cos(a) * r).toFloat()
        val dy = (sin(a) * r).toFloat()
        drawLine(color, Offset(center.x - dx, center.y - dy), Offset(center.x + dx, center.y + dy), width, StrokeCap.Round)
    }
}
