package io.github.hyeonsikoh.sidedesk.rest

import io.github.hyeonsikoh.sidedesk.BuildConfig
import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import io.github.hyeonsikoh.sidedesk.ai.alertText
import io.github.hyeonsikoh.sidedesk.clock.ClockState
import io.github.hyeonsikoh.sidedesk.clock.HomeBig
import io.github.hyeonsikoh.sidedesk.clock.rememberNow
import io.github.hyeonsikoh.sidedesk.ui.LocalPalette
import io.github.hyeonsikoh.sidedesk.ui.Palette
import io.github.hyeonsikoh.sidedesk.ui.RestColors
import io.github.hyeonsikoh.sidedesk.ui.fit

/** 쉬는 판이 다 덮이는 데 걸리는 시간. 그 뒤로는 아래 화면을 구성하지 않는다. */
const val REST_FADE_IN_MS = 600

/**
 * 쉬는 화면. 화면 틀 위에 검은 판을 겹쳐 고른 시계 모양만 어둡게 그린다.
 * 모든 누름을 먼저 받아 깨우기만 하고, 아래 칸과 버튼에는 보내지 않는다. 나가는 0.2초 동안도 같다.
 */
@Composable
fun BoxScope.RestScreen(resting: Boolean, rest: RestState, clock: ClockState, time: () -> Long, onWake: () -> Unit) {
    BackHandler(enabled = resting, onBack = onWake)
    AnimatedVisibility(
        visible = resting,
        enter = fadeIn(tween(REST_FADE_IN_MS)),
        exit = fadeOut(tween(200)),
        modifier = Modifier.matchParentSize(),
    ) {
        Box(
            Modifier
                .fillMaxSize()
                .background(Color.Black)
                .pointerInput(Unit) {
                    awaitEachGesture {
                        awaitFirstDown(requireUnconsumed = false).consume()
                        onWake()
                        do {
                            val event = awaitPointerEvent(PointerEventPass.Main)
                            event.changes.forEach { it.consume() }
                        } while (event.changes.any { it.pressed })
                    }
                },
        ) {
            // 시각은 쉬는 판 안에서만 읽는다. 깨어 있을 때 1초마다 화면 전체가 다시 그려지지 않게 한다.
            val now = rememberNow(time)
            val (x, y) = shiftAt(now / 60_000)
            CompositionLocalProvider(LocalPalette provides RestColors) {
                Box(Modifier.fillMaxSize().padding(24.dp).offset(x.dp, y.dp)) {
                    HomeBig(clock.copy(picking = false, via = null), now, resting = true)
                }
                rest.alert?.let { alert ->
                    Text(
                        "${alert.title} / ${alertText(alert.kind)}",
                        color = Palette.ink,
                        fontSize = fit(14.dp),
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.align(Alignment.BottomCenter).padding(bottom = 28.dp, start = 24.dp, end = 24.dp),
                    )
                }
            }
        }
    }
}

/** 쉬기 설정 판에서 고르는 것 */
class RestPanel(
    val state: RestState,
    /** 세기를 바꿀 수 있는 폰인지 */
    val canSetStrength: Boolean,
    val setKeepOn: (KeepOn) -> Unit,
    val setRestAfter: (RestAfter) -> Unit,
    val setWakeOnAlert: (Boolean) -> Unit,
    val setVibration: (Vibration) -> Unit,
)

/** 홈 고르기 판 안의 쉬기 설정. 낮은 화면에서는 판 안에서 세로로 민다. */
@Composable
fun RestSettings(panel: RestPanel, modifier: Modifier = Modifier) {
    val s = panel.state
    Column(modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Line("켜 두기") { KeepOn.entries.forEach { Chip(it.label, it == s.keepOn) { panel.setKeepOn(it) } } }
        Line("쉬기") { RestAfter.entries.forEach { Chip(it.label, it == s.restAfter) { panel.setRestAfter(it) } } }
        Line("알림이 오면\n깨우기") {
            Chip("켬", s.wakeOnAlert) { panel.setWakeOnAlert(true) }
            Chip("끔", !s.wakeOnAlert) { panel.setWakeOnAlert(false) }
        }
        Line("진동") { Vibration.entries.forEach { Chip(it.label, it == s.vibration) { panel.setVibration(it) } } }
        if (!panel.canSetStrength) {
            Text("이 폰은 세기를 바꿀 수 없습니다", color = Palette.muted, fontSize = fit(11.dp), modifier = Modifier.padding(start = 4.dp))
        }
        // 컴퓨터와 버전이 다르다는 안내를 받았을 때 자기 버전을 볼 곳
        Text("SideDesk ${BuildConfig.VERSION_NAME}", color = Palette.muted, fontSize = fit(11.dp), modifier = Modifier.padding(start = 4.dp))
    }
}

@Composable
private fun Line(label: String, chips: @Composable () -> Unit) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        // 긴 이름(알림이 오면 깨우기)은 낱말 사이에서 두 줄로 나눠 적는다(한글은 글자 사이에서 끊기므로).
        Text(label, color = Palette.muted, fontSize = fit(12.dp), lineHeight = fit(14.dp), maxLines = 2, modifier = Modifier.width(84.dp).padding(end = 6.dp))
        Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(6.dp)) { chips() }
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
            .padding(horizontal = 14.dp, vertical = 7.dp),
    )
}
