package io.github.hyeonsikoh.sidedesk.shortcut

import androidx.compose.ui.text.font.FontWeight
import androidx.compose.animation.core.animateFloatAsState
import io.github.hyeonsikoh.sidedesk.ui.Track
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.scale
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import io.github.hyeonsikoh.sidedesk.link.Shortcut
import io.github.hyeonsikoh.sidedesk.link.Toggle
import io.github.hyeonsikoh.sidedesk.ui.Palette
import io.github.hyeonsikoh.sidedesk.ui.fit
import kotlin.math.ceil
import kotlin.math.min

private const val DIM = 0.4f
private const val GAP = 12f

/** 격자 모양. 가로 판은 4열 3줄, 세로 판은 3열 4줄(시안 .fd). 넘치면 줄이 늘고 민다. */
data class Grid(val columns: Int, val rows: Int, val cell: Float, val scrolls: Boolean)

fun gridOf(width: Float, height: Float, count: Int): Grid {
    val wide = width >= height
    val columns = if (wide) 4 else 3
    val baseRows = if (wide) 3 else 4
    val cell = min((width - GAP * (columns - 1)) / columns, (height - GAP * (baseRows - 1)) / baseRows)
    val rows = maxOf(baseRows, ceil(count / columns.toFloat()).toInt())
    return Grid(columns, rows, cell, rows > baseRows)
}

/** 단축키 큰 화면. 누르면 번호만 보내고, 실패할 때만 판 아래에 한 줄 보인다. */
@Composable
fun ShortcutBig(state: ShortcutState, onPress: (Int) -> Unit, modifier: Modifier = Modifier) {
    Track("keys-big")
    BoxWithConstraints(modifier.fillMaxSize()) {
        val items = state.items
        val width = maxWidth.value
        val height = maxHeight.value
        Box(Modifier.fillMaxSize().alpha(if (state.dimmed) DIM else 1f)) {
            if (items.isNullOrEmpty()) {
                if (items != null) {
                    Text(
                        "컴퓨터의 SideDesk 설정에서 단축키를 더하세요",
                        color = Palette.muted,
                        fontSize = fit(15.dp),
                        textAlign = TextAlign.Center,
                        modifier = Modifier.align(Alignment.Center).padding(16.dp),
                    )
                }
            } else {
                Keys(items, gridOf(width, height, items.size), onPress)
            }
        }
        state.notice?.let {
            Text(
                it,
                color = Palette.ground,
                fontSize = fit(14.dp),
                textAlign = TextAlign.Center,
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
private fun Keys(items: List<Shortcut>, grid: Grid, onPress: (Int) -> Unit) {
    val scroll = rememberScrollState()
    Box(Modifier.fillMaxSize()) {
        Column(
            Modifier.fillMaxSize().verticalScroll(scroll, enabled = grid.scrolls),
            verticalArrangement = Arrangement.spacedBy(GAP.dp, if (grid.scrolls) Alignment.Top else Alignment.CenterVertically),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            items.chunked(grid.columns).forEach { row ->
                Row(horizontalArrangement = Arrangement.spacedBy(GAP.dp)) {
                    row.forEach { Key(it, grid.cell.dp) { onPress(it.id) } }
                    repeat(grid.columns - row.size) { Box(Modifier.size(grid.cell.dp)) }
                }
            }
        }
        if (scroll.canScrollForward) {
            Box(
                Modifier
                    .align(Alignment.BottomCenter)
                    .fillMaxWidth()
                    .height(36.dp)
                    .background(Brush.verticalGradient(listOf(Color.Transparent, Palette.panel))),
            )
        }
    }
}

/**
 * 칸 하나(시안 .ky). 보통 버튼은 색 바탕 둥근 네모에 흰 그림, 시스템 버튼은 칸에 색 그림만,
 * 켜진 버튼은 칸 전체를 그 색으로 채운다. 누르면 0.9 로 줄었다 돌아온다.
 */
@Composable
private fun Key(item: Shortcut, size: Dp, onClick: () -> Unit) {
    val press = remember { MutableInteractionSource() }
    val pressed by press.collectIsPressedAsState()
    val scale by animateFloatAsState(if (pressed) 0.9f else 1f, label = "press")
    val color = shortcutColor(item.color)
    val on = item.toggle == Toggle.On
    val shape = RoundedCornerShape(size * 0.2f)
    Box(
        Modifier
            .size(size)
            .scale(scale)
            .clip(shape)
            .background(if (on) color else Palette.card)
            .clickable(press, indication = null, onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        val named = item.name.isNotBlank()
        val art = Modifier.padding(bottom = if (named) size * 0.12f else 0.dp)
        when {
            on -> Glyph(item.icon, item.name, onColor(color), art.size(size * 0.5f))
            item.system -> Glyph(item.icon, item.name, color, art.size(size * 0.5f))
            else -> Box(
                art.size(size * if (named) 0.6f else 0.74f).clip(RoundedCornerShape(size * 0.16f)).background(color),
                contentAlignment = Alignment.Center,
            ) {
                Glyph(item.icon, item.name, Color.White, Modifier.size(size * 0.42f))
            }
        }
        if (named) {
            Text(
                item.name,
                color = if (on) onColor(color).copy(alpha = 0.8f) else Palette.muted,
                fontSize = fit(min(12f, size.value * 0.12f).dp),
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                textAlign = TextAlign.Center,
                modifier = Modifier.align(Alignment.BottomCenter).padding(horizontal = 6.dp, vertical = size * 0.05f),
            )
        }
    }
}

/** 켜진 칸의 그림 색. 테마가 아니라 바탕 밝기로 정한다(노란 바탕이면 두 테마 모두 어두운 그림). */
fun onColor(background: Color): Color = if (background.luminance() > 0.4f) Color(0xFF1A1D22) else Color.White

/** 작은 칸(시안 .mdeck). 앞 네 개의 그림을 작게 놓는다. 좁으면 2×2 */
@Composable
fun ShortcutTile(state: ShortcutState, modifier: Modifier) {
    Track("keys-tile")
    BoxWithConstraints(modifier.clip(RoundedCornerShape(20.dp)).background(Palette.side).padding(horizontal = 14.dp, vertical = 12.dp), contentAlignment = Alignment.Center) {
        val first = state.items.orEmpty().take(4)
        if (first.isEmpty()) {
            Text("단축키", color = Palette.ink, fontSize = fit(16.dp), fontWeight = FontWeight.Medium, maxLines = 1)
            return@BoxWithConstraints
        }
        // 한 줄(폭이 높이의 1.6배 이상)이면 넷을 30 으로, 아니면 2×2 를 36(폭의 32% 까지), 낮으면 28.
        val row = maxWidth >= maxHeight * 1.6f
        val low = maxHeight <= 92.dp && !row
        val icon = when {
            row -> 30.dp
            low -> 28.dp
            else -> minOf(36.dp, maxWidth * 0.32f)
        }
        val gap = if (row) 8.dp else if (low) 7.dp else 10.dp
        Box(Modifier.alpha(if (state.dimmed) DIM else 1f)) {
            if (row) {
                Row(horizontalArrangement = Arrangement.spacedBy(gap)) { first.forEach { Mini(it, icon) } }
            } else {
                Column(verticalArrangement = Arrangement.spacedBy(gap)) {
                    first.chunked(2).forEach { pair -> Row(horizontalArrangement = Arrangement.spacedBy(if (low) 12.dp else gap)) { pair.forEach { Mini(it, icon) } } }
                }
            }
        }
    }
}

@Composable
private fun Mini(item: Shortcut, size: Dp) {
    val color = shortcutColor(item.color)
    if (item.system && item.toggle != Toggle.On) {
        Glyph(item.icon, item.name, color, Modifier.size(size))
    } else {
        Box(Modifier.size(size).clip(RoundedCornerShape(size * 0.22f)).background(color), contentAlignment = Alignment.Center) {
            Glyph(item.icon, item.name, if (item.toggle == Toggle.On) onColor(color) else Color.White, Modifier.size(size * 0.62f))
        }
    }
}
