package io.github.hyeonsikoh.sidedesk.ai

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
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
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.scale
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import io.github.hyeonsikoh.sidedesk.link.AiSession
import io.github.hyeonsikoh.sidedesk.link.AiState
import io.github.hyeonsikoh.sidedesk.link.AiTool
import io.github.hyeonsikoh.sidedesk.link.AlertKind
import io.github.hyeonsikoh.sidedesk.link.ForgeState
import io.github.hyeonsikoh.sidedesk.ui.Palette
import io.github.hyeonsikoh.sidedesk.ui.fit
import kotlin.math.ceil
import kotlin.math.min

private const val DIM = 0.4f
private val GAP = 8.dp

/** 세션 카드가 이보다 작아지면 줄이지 않고 목록을 민다. */
private const val MIN_UNIT = 56f

fun stateText(state: AiState) = when (state) {
    AiState.Working -> "작업 중"
    AiState.Waiting -> "답변 대기"
    AiState.Done -> "끝남"
    AiState.Unknown -> "알 수 없음"
}

fun alertText(kind: AlertKind) = if (kind == AlertKind.Waiting) "답변 대기" else "작업 끝남"

private fun toolText(tool: AiTool) = when (tool) {
    AiTool.ClaudeCode -> "Claude"
    AiTool.Codex -> "Codex"
    AiTool.Unknown -> null
}

/** AI 큰 화면. 위는 세션 카드, 아래는 GitHub/GitLab 숫자 */
@Composable
fun AiBig(state: AiScreenState, now: Long, onTab: (ForgeTab) -> Unit, modifier: Modifier = Modifier) {
    Column(modifier.fillMaxSize()) {
        BoxWithConstraints(Modifier.weight(1f).fillMaxWidth().alpha(if (state.dimmed) DIM else 1f)) {
            if (state.sessions.isEmpty()) {
                if (state.busy != null) {
                    Text(
                        "실행 중인 AI 세션이 없습니다",
                        color = Palette.muted,
                        fontSize = fit(15.dp),
                        textAlign = TextAlign.Center,
                        modifier = Modifier.align(Alignment.Center),
                    )
                }
            } else {
                Sessions(state, now, maxHeight)
            }
        }
        Spacer(Modifier.height(10.dp))
        ForgeBar(state, onTab)
    }
}

/**
 * 작업 중과 답변 대기는 판 너비 전체(높이 1.55u), 나머지는 반 너비 둘씩(높이 u).
 * u 는 100dp 를 넘지 않고, 다 넣으려면 56dp 아래가 될 때는 56dp 로 두고 민다.
 */
@Composable
private fun Sessions(state: AiScreenState, now: Long, height: Dp) {
    val active = state.sessions.filter { it.active() }
    val rest = state.sessions.filterNot { it.active() }
    val rows = active.size * 1.55f + ceil(rest.size / 2f)
    val count = active.size + ceil(rest.size / 2f)
    val unit = min(100f, (height.value - (count - 1) * GAP.value) / rows).coerceAtLeast(MIN_UNIT)
    val highlight = state.alert?.sessionId
    val scroll = rememberScrollState()
    Box(Modifier.fillMaxSize()) {
    Column(Modifier.fillMaxSize().verticalScroll(scroll), verticalArrangement = Arrangement.spacedBy(GAP)) {
        active.forEach { SessionCard(it, now, it.id == highlight, big = true, Modifier.fillMaxWidth().height((unit * 1.55f).dp)) }
        rest.chunked(2).forEach { pair ->
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(GAP)) {
                pair.forEach { SessionCard(it, now, it.id == highlight, big = false, Modifier.weight(1f).height(unit.dp)) }
                if (pair.size == 1) Spacer(Modifier.weight(1f))
            }
        }
    }
    // 아래로 더 있으면 끝을 판 색으로 흐리게 사라지게 해, 잘린 것이 아니라 밀어 볼 수 있다는 것을 알린다.
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

@Composable
private fun SessionCard(session: AiSession, now: Long, highlight: Boolean, big: Boolean, modifier: Modifier) {
    val shape = RoundedCornerShape(if (big) 20.dp else 16.dp)
    val (background, edge) = when (session.state) {
        AiState.Working -> Palette.hot to Palette.accent.copy(alpha = 0.35f)
        AiState.Waiting -> Palette.waiting.copy(alpha = 0.16f).compositeOver(Palette.card) to Palette.waiting.copy(alpha = 0.6f)
        else -> Palette.card to Color.Transparent
    }
    Column(
        modifier
            .clip(shape)
            .background(background)
            .border(if (highlight) 2.dp else 1.dp, if (highlight) Palette.accent else edge, shape)
            .padding(horizontal = 14.dp, vertical = if (big) 12.dp else 8.dp)
            .alpha(if (session.open) 1f else 0.6f),
        verticalArrangement = Arrangement.Center,
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                session.title,
                color = Palette.ink,
                fontSize = fit(if (big) 19.dp else 15.dp),
                fontWeight = FontWeight.SemiBold,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f),
            )
            Text(elapsedText(now, session.since), color = Palette.muted, fontSize = fit(12.dp), maxLines = 1)
        }
        Row(Modifier.padding(top = 2.dp), verticalAlignment = Alignment.CenterVertically) {
            toolText(session.tool)?.let {
                Text(
                    it,
                    color = Palette.muted,
                    fontSize = fit(10.dp),
                    maxLines = 1,
                    modifier = Modifier
                        .padding(end = 6.dp)
                        .clip(RoundedCornerShape(6.dp))
                        .background(Palette.side)
                        .padding(horizontal = 5.dp, vertical = 1.dp),
                )
            }
            Text(
                listOf(stateText(session.state), session.project).filter { it.isNotEmpty() }.joinToString(", "),
                color = if (session.state == AiState.Waiting) Palette.waiting else Palette.muted,
                fontSize = fit(if (big) 13.dp else 12.dp),
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

private fun Color.compositeOver(under: Color): Color {
    val a = alpha
    return Color(red * a + under.red * (1 - a), green * a + under.green * (1 - a), blue * a + under.blue * (1 - a), 1f)
}

/** 아래 줄: GitHub/GitLab 고르기와 검토, 내 요청, 알림 수 */
@Composable
private fun ForgeBar(state: AiScreenState, onTab: (ForgeTab) -> Unit) {
    Row(
        Modifier.fillMaxWidth().height(64.dp).clip(RoundedCornerShape(18.dp)).background(Palette.card).padding(horizontal = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Row(Modifier.clip(RoundedCornerShape(14.dp)).background(Palette.side).padding(3.dp)) {
            ForgeTab.entries.forEach { tab ->
                val on = tab == state.tab
                Text(
                    tab.name,
                    color = if (on) Palette.panel else Palette.ink,
                    fontSize = fit(13.dp),
                    fontWeight = if (on) FontWeight.SemiBold else FontWeight.Normal,
                    maxLines = 1,
                    modifier = Modifier
                        .clip(RoundedCornerShape(11.dp))
                        .background(if (on) Palette.accent else Color.Transparent)
                        .clickable { onTab(tab) }
                        .padding(horizontal = 12.dp, vertical = 8.dp),
                )
            }
        }
        Box(Modifier.weight(1f).alpha(if (state.dimmed) DIM else 1f), contentAlignment = Alignment.Center) {
            val counts = state.counts
            if (counts?.state == ForgeState.Ok) {
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceEvenly) {
                    Count("${counts.reviews}", "검토")
                    Count("${counts.mine}", "내 요청")
                    Count(noticeText(counts.notices), "알림")
                }
            } else {
                val tool = if (state.tab == ForgeTab.GitHub) "gh" else "glab"
                Text(
                    when (counts?.state) {
                        ForgeState.Missing -> "$tool 가 설치되어 있지 않습니다"
                        ForgeState.SignedOut -> "$tool auth login 으로 로그인하세요"
                        ForgeState.Failed -> "불러오지 못했습니다"
                        else -> "-"
                    },
                    color = Palette.muted,
                    fontSize = fit(13.dp),
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.padding(horizontal = 8.dp),
                )
            }
        }
    }
}

@Composable
private fun Count(value: String, label: String) {
    Column(horizontalAlignment = Alignment.CenterHorizontally) {
        Text(value, color = Palette.ink, fontSize = fit(20.dp), fontWeight = FontWeight.Medium, maxLines = 1)
        Text(label, color = Palette.muted, fontSize = fit(11.dp), maxLines = 1)
    }
}

/**
 * AI 작은 칸. "AI n 작업 중", 일이 있으면 hot 바탕, 답변 대기가 있으면 주황 점.
 * 알림이 오면 살짝 커졌다 돌아오고 4초 동안 제목과 바뀐 상태를 보인다.
 */
@Composable
fun AiTile(state: AiScreenState, modifier: Modifier) {
    val pulse = remember { Animatable(1f) }
    val alert = state.alert
    LaunchedEffect(alert?.id) {
        if (alert != null) {
            pulse.animateTo(1.06f, tween(140))
            pulse.animateTo(1f, tween(220))
        }
    }
    val busy = state.busy ?: 0
    Box(
        modifier
            .scale(pulse.value)
            .clip(RoundedCornerShape(20.dp))
            .background(if (busy > 0 || alert != null) Palette.hot else Palette.side)
            .padding(horizontal = 12.dp),
    ) {
        if (alert != null) {
            Column(Modifier.align(Alignment.CenterStart)) {
                Text(alert.title, color = Palette.ink, fontSize = fit(13.dp), fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(
                    alertText(alert.kind),
                    color = if (alert.kind == AlertKind.Waiting) Palette.waiting else Palette.muted,
                    fontSize = fit(11.dp),
                    maxLines = 1,
                )
            }
        } else {
            Row(Modifier.align(Alignment.Center).alpha(if (state.dimmed) DIM else 1f), verticalAlignment = Alignment.CenterVertically) {
                Text("AI", color = Palette.ink, fontSize = fit(20.dp), fontWeight = FontWeight.Bold, maxLines = 1)
                Spacer(Modifier.weight(1f))
                Text(state.busy?.toString() ?: "-", color = Palette.ink, fontSize = fit(26.dp), fontWeight = FontWeight.Medium, maxLines = 1)
                Text(" 작업 중", color = Palette.muted, fontSize = fit(10.dp), maxLines = 1)
            }
            if (state.waiting) {
                Box(Modifier.align(Alignment.TopEnd).padding(top = 10.dp).size(7.dp).clip(CircleShape).background(Palette.waiting))
            }
        }
    }
}
