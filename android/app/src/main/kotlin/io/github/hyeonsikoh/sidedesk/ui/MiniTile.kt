package io.github.hyeonsikoh.sidedesk.ui

import androidx.compose.ui.unit.em
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.material3.Text
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp

/** 작은 칸의 모양. 글자 크기는 칸 크기가 아니라 이 셋으로만 바뀐다. */
data class MiniSize(
    /** 폭이 높이의 1.6배 이상: 한 줄로 놓는다 */
    val wide: Boolean,
    /** 폭 130 이하 */
    val narrow: Boolean,
    /** 높이 92 이하이고 한 줄이 아님 */
    val low: Boolean,
) {
    /** 큰 숫자 */
    val num: Int get() = if (narrow) 27 else 30
    /** 이름 */
    val name: Int get() = if (narrow) 15 else 16

    companion object {
        /** 이름(16), 라벨(14), 단위(15), AI 글자(22) */
        const val LABEL = 14
        const val UNIT = 15
        const val MARK = 22

        fun of(width: Float, height: Float) = MiniSize(
            wide = width >= height * 1.6f,
            narrow = width <= 130f,
            low = height <= 92f && width < height * 1.6f,
        )
    }
}

/**
 * 작은 칸 틀. 첫째는 위 왼쪽, 둘째는 아래 왼쪽(위아래로 벌림). 한 줄이면 양 끝으로 벌린다.
 * `behind` 는 글 아래에 깔리는 것(물결 등).
 */
@Composable
fun MiniTile(
    modifier: Modifier,
    background: Color = Palette.side,
    behind: @Composable BoxScope.() -> Unit = {},
    first: @Composable (MiniSize) -> Unit,
    second: @Composable (MiniSize) -> Unit,
) {
    BoxWithConstraints(modifier.clip(RoundedCornerShape(20.dp)).background(background)) {
        val size = MiniSize.of(maxWidth.value, maxHeight.value)
        behind()
        val padding = when {
            size.low -> PaddingValues(horizontal = 11.dp, vertical = 9.dp)
            size.narrow -> PaddingValues(horizontal = 12.dp, vertical = 11.dp)
            else -> PaddingValues(horizontal = 14.dp, vertical = 12.dp)
        }
        if (size.wide) {
            Row(
                Modifier.fillMaxSize().padding(padding),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                first(size)
                Spacer(Modifier.weight(1f))
                second(size)
            }
        } else {
            Column(
                Modifier.fillMaxSize().padding(padding),
                verticalArrangement = Arrangement.SpaceBetween,
                horizontalAlignment = Alignment.Start,
            ) {
                first(size)
                second(size)
            }
        }
    }
}

/** 작은 칸의 큰 숫자. 단위(%, 등)는 15 로 붙인다. */
@Composable
fun MiniNum(text: String, unit: String?, size: MiniSize, color: Color = Palette.ink) {
    Text(
        buildAnnotatedString {
            append(text)
            if (unit != null) withStyle(SpanStyle(fontSize = fit(MiniSize.UNIT.dp))) { append(unit) }
        },
        color = color,
        fontSize = fit(size.num.dp),
        fontWeight = FontWeight.Medium,
        letterSpacing = (-0.03).em,
        lineHeight = fit(size.num.dp),
        maxLines = 1,
        softWrap = false,
    )
}

/** 작은 칸의 흐린 라벨(14) */
@Composable
fun MiniLabel(text: String) {
    Text(text, color = Palette.muted, fontSize = fit(MiniSize.LABEL.dp), fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis)
}

/** 작은 칸의 이름(16, 좁으면 15). 낮은 칸은 한 줄, 아니면 두 줄까지 */
@Composable
fun MiniName(text: String, size: MiniSize, modifier: Modifier = Modifier) {
    Text(
        text,
        color = Palette.ink,
        fontSize = fit(size.name.dp),
        fontWeight = FontWeight.Medium,
        lineHeight = fit((size.name * 1.22f).dp),
        maxLines = if (size.low || size.wide) 1 else 2,
        overflow = TextOverflow.Ellipsis,
        modifier = modifier,
    )
}
