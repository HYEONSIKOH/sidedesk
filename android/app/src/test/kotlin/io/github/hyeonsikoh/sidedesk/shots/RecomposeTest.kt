package io.github.hyeonsikoh.sidedesk.shots

import androidx.activity.ComponentActivity
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.unit.dp
import io.github.hyeonsikoh.sidedesk.Models
import io.github.hyeonsikoh.sidedesk.SideDeskScreen
import io.github.hyeonsikoh.sidedesk.ai.AiViewModel
import io.github.hyeonsikoh.sidedesk.ai.Buzzer
import io.github.hyeonsikoh.sidedesk.clock.ClockViewModel
import io.github.hyeonsikoh.sidedesk.clock.FakeSettings
import io.github.hyeonsikoh.sidedesk.desk.DeskViewModel
import io.github.hyeonsikoh.sidedesk.desk.Screen
import io.github.hyeonsikoh.sidedesk.home.HomeViewModel
import io.github.hyeonsikoh.sidedesk.link.AiSession
import io.github.hyeonsikoh.sidedesk.link.AiState
import io.github.hyeonsikoh.sidedesk.link.AiTool
import io.github.hyeonsikoh.sidedesk.link.AlertKind
import io.github.hyeonsikoh.sidedesk.link.FakeLink
import io.github.hyeonsikoh.sidedesk.link.LinkState
import io.github.hyeonsikoh.sidedesk.link.Perf
import io.github.hyeonsikoh.sidedesk.link.Processes
import io.github.hyeonsikoh.sidedesk.link.Via
import io.github.hyeonsikoh.sidedesk.music.MusicViewModel
import io.github.hyeonsikoh.sidedesk.rest.RestViewModel
import io.github.hyeonsikoh.sidedesk.shortcut.ShortcutViewModel
import io.github.hyeonsikoh.sidedesk.ui.Confirm
import io.github.hyeonsikoh.sidedesk.ui.LocalTrack
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.RuleChain
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.util.TimeZone

/**
 * 앱과 같은 구성(SideDeskScreen)을 가짜 연결로 띄워, 값이 그대로인 10초 동안
 * 칸마다 다시 구성된 횟수와 그린 프레임 수를 센다(SD-14).
 * 시각은 시험 시계에 묶는다. 시작은 hh:mm:05 라 10초 안에 분 경계가 없다.
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [34], qualifiers = "w914dp-h411dp")
class RecomposeTest {
    private val compose = createAndroidComposeRule<ComponentActivity>()

    @get:Rule
    val rules: RuleChain = RuleChain.outerRule(registerActivity).around(compose)

    private val zone = TimeZone.getDefault()
    private val link = FakeLink()
    private val counts = mutableMapOf<String, Int>()
    private var frames = 0

    /** 시험 시계가 0 일 때의 벽시계. start 가 정한다. */
    private var base = 0L

    @After
    fun tearDown() = TimeZone.setDefault(zone)

    /**
     * 화면을 띄우고 1초 흘린 뒤 세기를 비운다. 세기 시작은 2026-11-25 10:26 의 [second] 초다.
     * 기본 5초라 이어지는 10초 안에 분 경계가 없다.
     */
    private fun start(screen: Screen, resting: Boolean = false, playing: Boolean = false, second: Int = 5): Models {
        TimeZone.setDefault(TimeZone.getTimeZone("Asia/Seoul"))
        compose.mainClock.autoAdvance = false
        val warmup = if (resting) 62_000L else 1_000L
        base = DeskShots.NOW + second * 1_000L - warmup - compose.mainClock.currentTime
        link.state.value = LinkState.Connected("맥", Via.Usb)
        link.weather.value = DeskShots.SUNNY
        link.perf.value = Perf(31f, 12f, 58f, 44f, 18_000, 6_000)
        link.processes.value = Processes(DeskShots.ROWS)
        link.sessions.value = DeskShots.SESSIONS.map { it.copy(since = DeskShots.NOW - 60_000) }
        link.shortcuts.value = DeskShots.KEYS.items
        link.nowPlaying.value = DeskShots.SONG.now!!.copy(receivedAt = 0, playing = playing)
        val settings = FakeSettings(restAfter = if (resting) "One" else "Never")
        val quiet = object : Buzzer {
            override fun buzz(kind: AlertKind) = Unit
        }
        val ai = AiViewModel(link, quiet) { base }
        val models = Models(
            HomeViewModel(link),
            DeskViewModel(link) { 1L },
            ai,
            ShortcutViewModel(link, quiet) { 1L },
            MusicViewModel(link, quiet, { 1L }) { compose.mainClock.currentTime },
            ClockViewModel(link, settings) { base },
            RestViewModel(settings, ai.state),
            canSetStrength = true,
            wall = { base + compose.mainClock.currentTime },
            mono = { compose.mainClock.currentTime },
            askingLocation = mutableStateOf(false),
            locationNotice = Confirm("", "", ""),
            answerLocation = {},
        )
        models.desk.show(screen)
        models.rest.setVisible(true)
        compose.setContent {
            CompositionLocalProvider(LocalTrack provides { name: String -> counts[name] = (counts[name] ?: 0) + 1 }) {
                SideDeskScreen(models)
            }
        }
        step(warmup)
        counts.clear()
        frames = 0
        return models
    }

    /** 시험 시계와 메인 루퍼를 함께 100ms 씩 민다(ViewModel 수집은 메인 루퍼에서 돈다). */
    private fun step(ms: Long = 100) {
        repeat((ms / 100).toInt()) {
            compose.mainClock.advanceTimeBy(100)
            org.robolectric.shadows.ShadowLooper.idleMainLooper(100, java.util.concurrent.TimeUnit.MILLISECONDS)
            if (androidx.compose.runtime.Recomposer.runningRecomposers.value.any { it.hasPendingWork }) frames += 1
        }
    }

    // Robolectric 은 화면을 실제로 그리지 않아 그린 횟수를 셀 수 없다.
    // 그래서 100ms 마다 다음 프레임을 기다리는 일(애니메이션, 다시 구성)이 남았는지를 프레임 수로 센다.

    private fun idle(seconds: Int = 10) = step(seconds * 1_000L)

    private fun report(label: String): Map<String, Int> {
        println("RECOMPOSE $label frames=$frames ${counts.toSortedMap()}")
        return counts.toMap()
    }

    @Test
    fun countsAPositiveControl() {
        compose.mainClock.autoAdvance = false
        compose.setContent {
            Box(Modifier.fillMaxSize()) {
                val phase by rememberInfiniteTransition(label = "t").animateFloat(0f, 1f, infiniteRepeatable(tween(1_000)), label = "p")
                Canvas(Modifier.size(10.dp)) { drawCircle(Color.Red, size.minDimension * phase) }
            }
        }
        step(100)
        frames = 0
        idle(2)
        println("RECOMPOSE positive-control frames=$frames")
        assertTrue("무한 애니메이션이면 프레임이 세어져야 한다", frames > 0)
    }

    /** 값과 분이 그대로인 10초: 아무 칸도 다시 구성되지 않고, 기다리는 프레임도 없다. */
    private fun assertStill(screen: Screen) {
        start(screen)
        idle()
        val counts = report("idle-${screen.name.lowercase()}")
        assertEquals(emptyMap<String, Int>(), counts)
        assertEquals(0, frames)
    }

    @Test fun idleHome() = assertStill(Screen.Home)

    @Test fun idleAi() = assertStill(Screen.Ai)

    @Test fun idlePerf() = assertStill(Screen.Perf)

    @Test fun idleProcesses() = assertStill(Screen.Processes)

    @Test fun idleShortcuts() = assertStill(Screen.Shortcuts)

    @Test fun idleMusic() = assertStill(Screen.Music)

    @Test
    fun idleResting() {
        start(Screen.Perf, resting = true)
        idle()
        assertEquals(emptyMap<String, Int>(), report("idle-resting"))
        assertEquals(0, frames)
    }

    @Test
    fun aMinuteBoundaryRecomposesOnlyTheClock() {
        start(Screen.Home, second = 55)
        idle()
        assertEquals(mapOf("home-big" to 1), report("minute-home"))
    }

    @Test
    fun aMinuteBoundaryOnAiRecomposesOnlyTheTimes() {
        start(Screen.Ai, second = 55)
        idle()
        assertEquals(mapOf("ai-big" to 1, "home-tile" to 1), report("minute-ai"))
    }

    @Test
    fun playingMusicMovesOnlyTheMusic() {
        start(Screen.Music, playing = true)
        idle()
        val counts = report("playing-music")
        assertEquals(setOf("music-big"), counts.keys)
        // 0.5초마다(10초에 20번 안팎). 시험 시계와 루퍼를 함께 밀어 한두 번 더 셀 수 있다.
        assertTrue(counts.getValue("music-big") in 20..22)
    }

    @Test
    fun playingMusicInATileMovesOnlyTheTile() {
        start(Screen.Home, playing = true)
        idle()
        val counts = report("playing-tile")
        assertEquals(setOf("music-tile"), counts.keys)
        assertTrue(counts.getValue("music-tile") in 20..22)
    }

    @Test
    fun aPerfValueRecomposesOnlyThePerfTile() {
        start(Screen.Home)
        link.perf.value = Perf(40f, 12f, 58f, 44f, 18_000, 6_000)
        step(1_000)
        val counts = report("perf-change")
        for (other in listOf("home-big", "ai-tile", "music-tile", "keys-tile", "process-tile")) {
            assertEquals("$other 은 다시 구성되지 않아야 한다", null, counts[other])
        }
        assertEquals(1, counts["cpu-tile"])
        // 값이 바뀌면 물결이 0.6초 움직이고 멈춘다.
        frames = 0
        idle(2)
        assertEquals(0, frames)
    }
}
