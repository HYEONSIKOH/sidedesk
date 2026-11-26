package io.github.hyeonsikoh.sidedesk.shortcut

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathOperation
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.text.font.FontWeight
import io.github.hyeonsikoh.sidedesk.ui.fit
import kotlin.math.PI
import kotlin.math.cos
import kotlin.math.sin

/** 정해진 그림 묶음(be 와 같은 이름표). */
val GLYPHS = setOf(
    "terminal", "folder", "code", "browser", "calculator", "settings", "activity",
    "music", "volume", "dnd", "lock", "file", "star", "bolt",
)

/** 색 이름표의 실제 색. screens 넷째 그림에서 뽑았다. 모르면 gray */
fun shortcutColor(name: String): Color = when (name) {
    "slate" -> Color(0xFF4F7090)
    "green" -> Color(0xFF6C8A6E)
    "purple" -> Color(0xFF8A7AA8)
    "orange" -> Color(0xFFA8805A)
    "teal" -> Color(0xFF5E8A94)
    "olive" -> Color(0xFF7A8F5E)
    "rose" -> Color(0xFFA86A7C)
    "sand" -> Color(0xFFE0B95C)
    "sky" -> Color(0xFF7FA2D9)
    else -> Color(0xFF747C88)
}

/** 단축키 그림. 선으로만 그린다. 모르는 이름표는 이름 첫 글자다. */
@Composable
fun Glyph(icon: String, fallback: String, color: Color, modifier: Modifier) {
    if (icon !in GLYPHS) {
        BoxWithConstraints(modifier, contentAlignment = Alignment.Center) {
            Text(
                fallback.take(1).ifEmpty { "?" },
                color = color,
                fontSize = fit(maxHeight * 0.7f),
                fontWeight = FontWeight.SemiBold,
                maxLines = 1,
            )
        }
        return
    }
    Box(modifier) {
        Canvas(Modifier.fillMaxSize()) {
            val line = Stroke(size.minDimension * 0.08f, cap = StrokeCap.Round, join = StrokeJoin.Round)
            draw(icon, color, line)
        }
    }
}

/** 초승달: 원에서 오른쪽 위로 비킨 원을 뺀 모양 */
fun crescent(center: Offset, r: Float): Path {
    val disc = Path().apply { addOval(Rect(center, r)) }
    val bite = Path().apply { addOval(Rect(Offset(center.x + r * 0.5f, center.y - r * 0.35f), r * 0.85f)) }
    return Path().apply { op(disc, bite, PathOperation.Difference) }
}

private fun DrawScope.p(x: Float, y: Float) = Offset(size.width * x, size.height * y)

private fun DrawScope.path(color: Color, line: Stroke, vararg points: Pair<Float, Float>, close: Boolean = false) {
    val path = Path().apply {
        points.forEachIndexed { i, (x, y) -> if (i == 0) moveTo(size.width * x, size.height * y) else lineTo(size.width * x, size.height * y) }
        if (close) close()
    }
    drawPath(path, color, style = line)
}

private fun DrawScope.box(color: Color, line: Stroke, left: Float, top: Float, right: Float, bottom: Float, round: Float = 0.1f) {
    drawRoundRect(
        color,
        topLeft = p(left, top),
        size = Size(size.width * (right - left), size.height * (bottom - top)),
        cornerRadius = CornerRadius(size.minDimension * round),
        style = line,
    )
}

private fun DrawScope.draw(icon: String, color: Color, line: Stroke) {
    when (icon) {
        "terminal" -> {
            path(color, line, 0.2f to 0.3f, 0.42f to 0.5f, 0.2f to 0.7f)
            path(color, line, 0.5f to 0.72f, 0.8f to 0.72f)
        }
        "folder" -> path(color, line, 0.14f to 0.3f, 0.4f to 0.3f, 0.48f to 0.38f, 0.86f to 0.38f, 0.86f to 0.76f, 0.14f to 0.76f, close = true)
        "code" -> {
            path(color, line, 0.36f to 0.3f, 0.16f to 0.5f, 0.36f to 0.7f)
            path(color, line, 0.64f to 0.3f, 0.84f to 0.5f, 0.64f to 0.7f)
        }
        "browser" -> {
            val c = p(0.5f, 0.5f)
            val r = size.minDimension * 0.34f
            drawCircle(color, r, c, style = line)
            drawOval(color, Offset(c.x - r * 0.45f, c.y - r), Size(r * 0.9f, r * 2), style = line)
            drawLine(color, Offset(c.x - r, c.y), Offset(c.x + r, c.y), line.width, StrokeCap.Round)
        }
        "calculator" -> {
            box(color, line, 0.24f, 0.14f, 0.76f, 0.86f)
            box(color, line, 0.34f, 0.24f, 0.66f, 0.38f, 0.03f)
            for (row in 0..1) for (col in 0..1) {
                drawCircle(color, line.width * 0.7f, p(0.4f + col * 0.2f, 0.54f + row * 0.18f))
            }
        }
        "settings" -> for ((i, knob) in listOf(0.35f, 0.65f, 0.45f).withIndex()) {
            val y = 0.3f + i * 0.2f
            path(color, line, 0.16f to y, 0.84f to y)
            drawCircle(color, line.width * 1.3f, p(knob, y))
        }
        "activity" -> path(color, line, 0.12f to 0.52f, 0.32f to 0.52f, 0.42f to 0.3f, 0.56f to 0.72f, 0.66f to 0.52f, 0.88f to 0.52f)
        "music" -> {
            path(color, line, 0.4f to 0.66f, 0.4f to 0.24f, 0.76f to 0.18f, 0.76f to 0.6f)
            drawCircle(color, size.minDimension * 0.09f, p(0.32f, 0.68f), style = line)
            drawCircle(color, size.minDimension * 0.09f, p(0.68f, 0.62f), style = line)
        }
        "volume" -> {
            path(color, line, 0.16f to 0.4f, 0.3f to 0.4f, 0.48f to 0.24f, 0.48f to 0.76f, 0.3f to 0.6f, 0.16f to 0.6f, close = true)
            drawArc(color, -45f, 90f, false, p(0.48f, 0.36f), Size(size.width * 0.2f, size.height * 0.28f), style = line)
            drawArc(color, -50f, 100f, false, p(0.46f, 0.24f), Size(size.width * 0.36f, size.height * 0.52f), style = line)
        }
        "dnd" -> {
            val r = size.minDimension * 0.3f
            val c = p(0.5f, 0.5f)
            drawPath(crescent(c, r), color, style = line)
        }
        "lock" -> {
            box(color, line, 0.24f, 0.44f, 0.76f, 0.84f)
            drawArc(color, 180f, 180f, false, p(0.33f, 0.18f), Size(size.width * 0.34f, size.height * 0.5f), style = line)
            path(color, line, 0.33f to 0.43f, 0.33f to 0.44f)
            path(color, line, 0.67f to 0.43f, 0.67f to 0.44f)
        }
        "file" -> path(color, line, 0.26f to 0.14f, 0.58f to 0.14f, 0.76f to 0.32f, 0.76f to 0.86f, 0.26f to 0.86f, close = true)
        "star" -> {
            val points = (0 until 10).map { i ->
                val r = if (i % 2 == 0) 0.36f else 0.15f
                val a = -PI / 2 + i * PI / 5
                (0.5f + (cos(a) * r).toFloat()) to (0.53f + (sin(a) * r).toFloat())
            }
            path(color, line, *points.toTypedArray(), close = true)
        }
        "bolt" -> path(color, line, 0.56f to 0.12f, 0.28f to 0.54f, 0.5f to 0.54f, 0.44f to 0.88f, 0.72f to 0.44f, 0.5f to 0.44f, close = true)
    }
}
