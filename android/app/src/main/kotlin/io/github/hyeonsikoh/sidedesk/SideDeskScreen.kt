package io.github.hyeonsikoh.sidedesk

import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.State
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.hyeonsikoh.sidedesk.ai.AiViewModel
import io.github.hyeonsikoh.sidedesk.ai.LocalPulsed
import io.github.hyeonsikoh.sidedesk.clock.ClockViewModel
import io.github.hyeonsikoh.sidedesk.clock.PickActions
import io.github.hyeonsikoh.sidedesk.desk.AiPart
import io.github.hyeonsikoh.sidedesk.desk.ClockPart
import io.github.hyeonsikoh.sidedesk.desk.DeskActions
import io.github.hyeonsikoh.sidedesk.desk.DeskScreen
import io.github.hyeonsikoh.sidedesk.desk.DeskViewModel
import io.github.hyeonsikoh.sidedesk.desk.MusicPart
import io.github.hyeonsikoh.sidedesk.desk.ShortcutPart
import io.github.hyeonsikoh.sidedesk.home.HomeScreen
import io.github.hyeonsikoh.sidedesk.home.HomeViewModel
import io.github.hyeonsikoh.sidedesk.music.MusicActions
import io.github.hyeonsikoh.sidedesk.music.MusicViewModel
import io.github.hyeonsikoh.sidedesk.rest.REST_FADE_IN_MS
import io.github.hyeonsikoh.sidedesk.rest.RestPanel
import io.github.hyeonsikoh.sidedesk.rest.RestScreen
import io.github.hyeonsikoh.sidedesk.rest.RestState
import io.github.hyeonsikoh.sidedesk.rest.RestViewModel
import io.github.hyeonsikoh.sidedesk.shortcut.ShortcutViewModel
import io.github.hyeonsikoh.sidedesk.ui.Confirm
import io.github.hyeonsikoh.sidedesk.ui.SideDeskTheme
import kotlinx.coroutines.delay

/** 앱 화면에 필요한 것. MainActivity 가 만들고, 시험은 가짜 연결로 만든다. */
class Models(
    val home: HomeViewModel,
    val desk: DeskViewModel,
    val ai: AiViewModel,
    val shortcuts: ShortcutViewModel,
    val music: MusicViewModel,
    val clock: ClockViewModel,
    val rest: RestViewModel,
    val canSetStrength: Boolean,
    /** 벽시계(유닉스 밀리초) */
    val wall: () -> Long,
    /** 부팅 뒤 시각(밀리초, 깊은 잠 포함), 음악 진행 바용 */
    val mono: () -> Long,
    val askingLocation: State<Boolean>,
    val locationNotice: Confirm,
    val answerLocation: (Boolean?) -> Unit,
)

/** 앱 화면 전체. 연결 화면(짝 맺기, 문제)이나 화면 틀, 그 위의 쉬는 화면 */
@Composable
fun SideDeskScreen(m: Models) {
    val clockState = m.clock.state.collectAsStateWithLifecycle().value
    val restState = m.rest.state.collectAsStateWithLifecycle().value
    SideDeskTheme(clockState.theme) {
        Box(
            Modifier
                .fillMaxSize()
                // 누름(down)마다 쉬기 타이머를 다시 잰다. 보기만 하고 소비하지 않는다.
                .pointerInput(Unit) {
                    awaitEachGesture {
                        awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
                        m.rest.touched()
                    }
                },
        ) {
            // 쉬는 판이 다 덮인 뒤에는 아래 화면을 구성하지 않는다(시각 읽기, 애니메이션, 다시 구성이 모두 멈춤).
            // 깨면 바로 다시 구성해, 쉬는 판이 0.2초 동안 걷히는 사이에 아래가 보인다.
            var covered by remember { mutableStateOf(false) }
            LaunchedEffect(restState.resting) {
                covered = false
                if (restState.resting) {
                    delay(REST_FADE_IN_MS.toLong())
                    covered = true
                }
            }
            val pulsed = remember { mutableStateOf<Long?>(null) }
            if (!covered) CompositionLocalProvider(LocalPulsed provides pulsed) { Desk(m, restState) }
            RestScreen(restState.resting, restState, clockState, m.wall, m.rest::touched)
        }
    }
}

/**
 * 연결 화면이나 화면 틀. 칸마다 묶음(Part)을 그 상태가 바뀔 때만 새로 만든다.
 * 그래야 한 값이 바뀌어도 다른 칸은 같은 묶음을 받아 건너뛴다.
 */
@Composable
private fun Desk(m: Models, restState: RestState) {
    val state = m.home.state.collectAsStateWithLifecycle().value
    if (state.pairingCode != null || state.problem != null) {
        HomeScreen(state, m.home::answer)
        return
    }
    val actions = remember {
        DeskActions(
            m.desk::show, m.desk::next, m.desk::previous, m.desk::sortBy, m.desk::ask, m.desk::cancel,
            m.desk::confirmKill, m.desk::dismissNotice,
        )
    }
    val pick = remember { PickActions(m.clock::setFace, m.clock::setTheme, m.clock::done) }
    val panel = remember(restState) {
        RestPanel(
            restState, m.canSetStrength,
            m.rest::setKeepOn, m.rest::setRestAfter, m.rest::setWakeOnAlert, m.rest::setVibration,
        )
    }
    val clockState = m.clock.state.collectAsStateWithLifecycle().value
    val notice = state.notice
    val part = remember(clockState, notice, panel) {
        ClockPart(clockState, m.wall, m.clock::pick, m.clock::nextFace, m.clock::previousFace, pick, notice, panel)
    }
    val aiState = m.ai.state.collectAsStateWithLifecycle().value
    val aiPart = remember(aiState) { AiPart(aiState, m.ai::selectTab) }
    val keysState = m.shortcuts.state.collectAsStateWithLifecycle().value
    val keysPart = remember(keysState) { ShortcutPart(keysState, m.shortcuts::press) }
    val musicActions = remember { MusicActions(m.music::playPause, m.music::next, m.music::previous, m.music::dragVolume, m.music::releaseVolume) }
    val musicState = m.music.state.collectAsStateWithLifecycle().value
    val musicPart = remember(musicState) { MusicPart(musicState, m.mono, musicActions) }
    DeskScreen(
        m.desk.state.collectAsStateWithLifecycle().value,
        part,
        actions,
        aiPart,
        keysPart,
        musicPart,
        modal = if (m.askingLocation.value) m.locationNotice else null,
        onModal = m.answerLocation,
    )
}
