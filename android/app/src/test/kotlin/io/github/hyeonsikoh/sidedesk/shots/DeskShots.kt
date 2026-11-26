package io.github.hyeonsikoh.sidedesk.shots

import androidx.compose.runtime.Composable
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.requiredSize
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.dp
import com.github.takahirom.roborazzi.captureRoboImage
import org.junit.Rule
import org.junit.rules.RuleChain
import io.github.hyeonsikoh.sidedesk.ai.AiScreenState
import io.github.hyeonsikoh.sidedesk.ai.ForgeTab
import io.github.hyeonsikoh.sidedesk.clock.ClockState
import io.github.hyeonsikoh.sidedesk.desk.AiPart
import io.github.hyeonsikoh.sidedesk.desk.ShortcutPart
import io.github.hyeonsikoh.sidedesk.rest.RestPanel
import io.github.hyeonsikoh.sidedesk.rest.RestScreen
import io.github.hyeonsikoh.sidedesk.rest.RestState
import io.github.hyeonsikoh.sidedesk.rest.shiftAt
import androidx.compose.foundation.layout.fillMaxSize
import io.github.hyeonsikoh.sidedesk.desk.MusicPart
import io.github.hyeonsikoh.sidedesk.link.MusicPlayer
import io.github.hyeonsikoh.sidedesk.link.NowPlaying
import io.github.hyeonsikoh.sidedesk.link.Volume
import io.github.hyeonsikoh.sidedesk.music.MusicActions
import io.github.hyeonsikoh.sidedesk.music.MusicState
import io.github.hyeonsikoh.sidedesk.link.Shortcut
import io.github.hyeonsikoh.sidedesk.link.Toggle
import io.github.hyeonsikoh.sidedesk.shortcut.ShortcutState
import io.github.hyeonsikoh.sidedesk.link.AiAlert
import io.github.hyeonsikoh.sidedesk.link.AiSession
import io.github.hyeonsikoh.sidedesk.link.AiState
import io.github.hyeonsikoh.sidedesk.link.AiTool
import io.github.hyeonsikoh.sidedesk.link.AlertKind
import io.github.hyeonsikoh.sidedesk.link.Forge
import io.github.hyeonsikoh.sidedesk.link.ForgeCounts
import io.github.hyeonsikoh.sidedesk.link.ForgeState
import io.github.hyeonsikoh.sidedesk.clock.Face
import io.github.hyeonsikoh.sidedesk.clock.PickActions
import io.github.hyeonsikoh.sidedesk.desk.ClockPart
import io.github.hyeonsikoh.sidedesk.desk.DeskActions
import io.github.hyeonsikoh.sidedesk.desk.DeskScreen
import io.github.hyeonsikoh.sidedesk.desk.DeskState
import io.github.hyeonsikoh.sidedesk.desk.Screen
import io.github.hyeonsikoh.sidedesk.link.Perf
import io.github.hyeonsikoh.sidedesk.link.ProcessInfo
import io.github.hyeonsikoh.sidedesk.link.Sky
import io.github.hyeonsikoh.sidedesk.link.Via
import io.github.hyeonsikoh.sidedesk.link.Weather
import io.github.hyeonsikoh.sidedesk.link.WeatherSource
import io.github.hyeonsikoh.sidedesk.ui.SideDeskTheme
import io.github.hyeonsikoh.sidedesk.ui.Theme
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.ParameterizedRobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.util.TimeZone

/** 접은 화면, 펼친 화면, 바형 크기마다 라이트와 다크로 찍어 android/app/screenshots 에 남긴다. 기준 그림과 비교하지 않는다. */
@RunWith(ParameterizedRobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [34], qualifiers = "w1000dp-h1000dp-xxhdpi")
class DeskShots(private val device: String, private val width: Int, private val height: Int) {
    companion object {
        /** 2026-11-25(수) 10:26 한국 시각 */
        const val NOW = 1_795_569_960_000L

        @JvmStatic
        @ParameterizedRobolectricTestRunner.Parameters(name = "{0}")
        fun sizes() = listOf(
            arrayOf("bar-portrait", 411, 914),
            arrayOf("bar-landscape", 914, 411),
            arrayOf("folded-portrait", 344, 882),
            arrayOf("folded-landscape", 882, 344),
            arrayOf("unfolded-portrait", 673, 841),
            arrayOf("unfolded-landscape", 841, 673),
        )

        val SUNNY = Weather(12f, Sky.Clear, 15f, 6f, true, NOW, WeatherSource.OpenMeteo)
        val RAIN_KMA = Weather(8f, Sky.Rain, 11f, 4f, true, NOW, WeatherSource.Kma)
        val PERF = Perf(31f, 12f, 58f, 44f, 18_000, 6_000)
        val ROWS = listOf(
            ProcessInfo(1, 1, "빌드", 11.8f, 424_000_000),
            ProcessInfo(2, 2, "편집기", 4.2f, 812_000_000),
            ProcessInfo(3, 3, "브라우저", 2.1f, 1_900_000_000),
            ProcessInfo(4, 4, "터미널", 0.8f, 186_000_000),
            ProcessInfo(5, 5, "음악", 0.4f, 240_000_000),
        )
        private val NONE = DeskActions({}, {}, {}, {}, {}, {}, {}, {})
        private val PICK = PickActions({}, {}, {})
        private const val MINUTE = 60_000L

        private fun ai(id: String, title: String, project: String, state: AiState, ago: Long, tool: AiTool = AiTool.ClaudeCode, open: Boolean = true) =
            AiSession(id, tool, title, project, state, NOW - ago * MINUTE, open)

        val SESSIONS = listOf(
            ai("1", "로그인 화면", "sidedesk", AiState.Working, 0),
            ai("2", "테스트 실행", "web", AiState.Working, 4, AiTool.Codex),
            ai("3", "문서 정리", "docs", AiState.Done, 12),
            ai("4", "검색 개선", "search", AiState.Done, 35),
            ai("5", "오류 수정", "api", AiState.Done, 60, open = false),
            ai("6", "번역", "app", AiState.Done, 120, AiTool.Codex),
        )
        private val waiting = ai("1", "로그인 화면", "sidedesk", AiState.Waiting, 1)
        val MANY = SESSIONS.take(2) + ai("9", "아주 긴 세션 제목이 칸을 넘어가는 경우", "long-project-name", AiState.Working, 2) +
            (10..18).map { ai("$it", "세션 $it", "p$it", if (it == 13) AiState.Unknown else AiState.Done, it * 10L) }

        private fun counts(state: ForgeState) = ForgeCounts(state, 0, 0, 0)

        val AI = AiScreenState(
            sessions = SESSIONS,
            busy = 2,
            forge = Forge(ForgeCounts(ForgeState.Ok, 0, 0, 2), ForgeCounts(ForgeState.SignedOut, 0, 0, 0)),
        )
        val ALERT = AiAlert(7, "3", "문서 정리", AlertKind.Done, NOW)

        /** 시계 옮김이 가장 끝(+12, +12)인 시각. NOW 에서 몇 분 뒤 */
        val EDGE: Long = (0L until 9L).map { NOW + it * 60_000 }.first { shiftAt(it / 60_000) == (12 to 12) }

        /** 진행 바용 단조 시각. 곡을 받은 때보다 6초 뒤 */
        const val NOW_MS = 1_006_000L

        val SONG = MusicState(
            now = NowPlaying(MusicPlayer.Spotify, "여름 바다", "파도 소리", "바다", 221_000, 0, true, 78_000, true, true, true, receivedAt = 1_000_000),
            volume = Volume(true, 45, false, true),
        )

        /** 시험용 앨범 그림. 두 색 띠를 jpeg 로 담는다 */
        val ART: ByteArray by lazy {
            val bitmap = android.graphics.Bitmap.createBitmap(64, 64, android.graphics.Bitmap.Config.ARGB_8888)
            val canvas = android.graphics.Canvas(bitmap)
            canvas.drawColor(0xFFB07A5A.toInt())
            canvas.drawRect(0f, 40f, 64f, 64f, android.graphics.Paint().apply { color = 0xFF3E6075.toInt() })
            java.io.ByteArrayOutputStream().also { bitmap.compress(android.graphics.Bitmap.CompressFormat.JPEG, 80, it) }.toByteArray()
        }

        private fun key(id: Int, icon: String, color: String, toggle: Toggle = Toggle.None, system: Boolean = false) =
            Shortcut(id, "", icon, color, toggle, system)

        /** 시안 넷째 그림의 열한 개 */
        val KEYS = ShortcutState(
            items = listOf(
                key(1, "terminal", "slate"), key(2, "folder", "green"), key(3, "code", "purple"), key(4, "browser", "orange"),
                key(5, "calculator", "teal"), key(6, "settings", "gray"), key(7, "activity", "olive"), key(8, "music", "rose"),
                key(9, "volume", "sand", Toggle.On, system = true), key(10, "dnd", "sky", Toggle.Off, system = true), key(11, "lock", "gray", system = true),
            ),
        )
        private val MORE = listOf(key(12, "file", "slate"), key(13, "star", "sand"), key(14, "bolt", "orange"), key(15, "mystery", "unknown"))
        private const val SHOT = "shot"
    }

    val compose = createComposeRule()

    @get:Rule
    val rules: RuleChain = RuleChain.outerRule(registerActivity).around(compose)

    /** 찍을 화면. 창을 다시 띄우지 않고 내용만 바꾼다. */
    private var current by mutableStateOf<(@Composable () -> Unit)?>(null)

    private val zone = TimeZone.getDefault()

    @org.junit.After
    fun tearDown() = TimeZone.setDefault(zone)

    @Before
    fun setUp() {
        // 물결과 1초 시계가 계속 돌아 그리기가 끝나지 않으므로 시계를 손으로만 돌린다.
        compose.mainClock.autoAdvance = false
        compose.setContent { current?.invoke() }
        TimeZone.setDefault(TimeZone.getTimeZone("Asia/Seoul"))
    }

    private fun desk(big: Screen = Screen.Home, dimmed: Boolean = false) = DeskState(
        big = big,
        perf = PERF,
        received = listOf(3_000L, 18_000L, 6_000L, 12_000L, 30_000L, 9_000L),
        sent = listOf(1_000L, 2_000L, 6_000L, 3_000L, 5_000L, 6_000L),
        rows = ROWS,
        dimmed = dimmed,
    )

    private fun clock(face: Face = Face.Digits, theme: Theme, weather: Weather? = SUNNY, dimmed: Boolean = false, picking: Boolean = false) =
        ClockState(face = face, theme = theme, picking = picking, via = if (dimmed) null else Via.Usb, weather = weather, weatherDimmed = dimmed)

    /** 기기 크기의 판에 그린다. 글자 배율은 Density 로 바꾼다. */
    private fun shot(
        name: String,
        theme: Theme,
        desk: DeskState,
        clock: ClockState,
        scale: Float = 1f,
        ai: AiScreenState = AI,
        keys: ShortcutState = KEYS,
        music: MusicState = SONG,
        rest: RestState? = null,
        restOpen: Boolean = false,
        now: Long = NOW,
    ) {
        current = {
            val density = LocalDensity.current
            CompositionLocalProvider(LocalDensity provides Density(density.density, scale)) {
                Box(Modifier.requiredSize(width.dp, height.dp).testTag(SHOT)) {
                    SideDeskTheme(theme) {
                        Box(Modifier.fillMaxSize()) {
                            val panel = RestPanel(rest ?: RestState(), canSetStrength = !restOpen || theme == Theme.Dark, {}, {}, {}, {})
                            DeskScreen(
                                desk,
                                ClockPart(clock, { now }, {}, {}, {}, PICK, rest = panel, restOpen = restOpen),
                                NONE,
                                AiPart(ai) {},
                                ShortcutPart(keys) {},
                                MusicPart(music, { NOW_MS }, MusicActions({}, {}, {}, {}, {})),
                            )
                            RestScreen(rest?.resting == true, rest ?: RestState(), clock, { now }) {}
                        }
                    }
                }
            }
        }
        Snapshot.sendApplyNotifications()
        repeat(3) { compose.mainClock.advanceTimeByFrame() }
        compose.mainClock.advanceTimeBy(1_000)
        compose.onNodeWithTag(SHOT).captureRoboImage("screenshots/$device/${theme.name.lowercase()}-$name.png")
    }

    @Test
    fun screens() {
        for (theme in Theme.entries) {
            Face.entries.forEach { face -> shot("home-${face.name.lowercase()}", theme, desk(), clock(face, theme)) }
            shot("home-picking", theme, desk(), clock(Face.Digits, theme, picking = true))
            shot("home-no-weather", theme, desk(), clock(Face.Digits, theme, weather = null))
            shot("home-kma-rain", theme, desk(), clock(Face.Weather, theme, weather = RAIN_KMA))
            shot("home-lost", theme, desk(dimmed = true), clock(Face.Overview, theme, dimmed = true))
            shot("perf", theme, desk(Screen.Perf), clock(theme = theme))
            shot("ai", theme, desk(Screen.Ai), clock(theme = theme))
            shot("ai-waiting", theme, desk(Screen.Ai), clock(theme = theme), ai = AI.copy(sessions = listOf(waiting) + SESSIONS.drop(1), waiting = true))
            shot("ai-empty", theme, desk(Screen.Ai), clock(theme = theme), ai = AiScreenState(sessions = emptyList(), busy = 0))
            shot("ai-many", theme, desk(Screen.Ai), clock(theme = theme), ai = AI.copy(sessions = MANY, busy = 3))
            shot("ai-gitlab-signed-out", theme, desk(Screen.Ai), clock(theme = theme), ai = AI.copy(tab = ForgeTab.GitLab))
            shot("ai-forge-missing", theme, desk(Screen.Ai), clock(theme = theme), ai = AI.copy(forge = Forge(counts(ForgeState.Missing), counts(ForgeState.Failed))))
            shot("ai-lost", theme, desk(Screen.Ai, dimmed = true), clock(theme = theme, dimmed = true), ai = AI.copy(dimmed = true))
            shot("ai-alert-card", theme, desk(Screen.Ai), clock(theme = theme), ai = AI.copy(alert = ALERT))
            shot("home-ai-alert", theme, desk(), clock(theme = theme), ai = AI.copy(alert = ALERT))
            shot("keys", theme, desk(Screen.Shortcuts), clock(theme = theme))
            shot("keys-empty", theme, desk(Screen.Shortcuts), clock(theme = theme), keys = ShortcutState(items = emptyList()))
            shot("keys-many", theme, desk(Screen.Shortcuts), clock(theme = theme), keys = KEYS.copy(items = KEYS.items!! + MORE))
            shot("keys-failed", theme, desk(Screen.Shortcuts), clock(theme = theme), keys = KEYS.copy(notice = "터미널을 실행하지 못했습니다. 앱을 찾지 못했습니다"))
            shot("keys-lost", theme, desk(Screen.Shortcuts, dimmed = true), clock(theme = theme, dimmed = true), keys = KEYS.copy(dimmed = true))
            shot("keys-named", theme, desk(Screen.Shortcuts), clock(theme = theme), keys = KEYS.copy(items = KEYS.items!!.map { it.copy(name = it.name.ifEmpty { "이름" }) }))
            shot("music", theme, desk(Screen.Music), clock(theme = theme))
            shot("music-art", theme, desk(Screen.Music), clock(theme = theme), music = SONG.copy(art = ART))
            shot("music-paused", theme, desk(Screen.Music), clock(theme = theme), music = SONG.copy(now = SONG.now!!.copy(playing = false)))
            shot("music-none", theme, desk(Screen.Music), clock(theme = theme), music = MusicState(now = SONG.now!!.copy(player = MusicPlayer.None, title = ""), volume = SONG.volume))
            shot("music-live", theme, desk(Screen.Music), clock(theme = theme), music = SONG.copy(now = SONG.now!!.copy(durationMs = 0)))
            shot("music-muted", theme, desk(Screen.Music), clock(theme = theme), music = SONG.copy(volume = Volume(true, 30, true, true)))
            shot("music-fixed", theme, desk(Screen.Music), clock(theme = theme), music = SONG.copy(volume = Volume(true, 60, false, false), now = SONG.now!!.copy(canNext = false, canPrevious = false)))
            shot("music-failed", theme, desk(Screen.Music), clock(theme = theme), music = SONG.copy(notice = "음악 앱 제어를 허용해 주세요. 설정 창의 안내를 보세요"))
            shot("music-lost", theme, desk(Screen.Music, dimmed = true), clock(theme = theme, dimmed = true), music = SONG.copy(dimmed = true, lostAt = NOW_MS))
            shot("music-long", theme, desk(Screen.Music), clock(theme = theme), music = SONG.copy(now = SONG.now!!.copy(title = "아주 긴 곡 제목이 두 줄을 넘어가면 줄임표로 잘려야 하는 경우의 예시", artist = "아주 긴 가수 이름 아주 긴 가수 이름")))
            Face.entries.forEach { face ->
                shot("rest-${face.name.lowercase()}", theme, desk(), clock(face, theme), rest = RestState(resting = true))
            }
            shot("rest-alert", theme, desk(), clock(Face.Digits, theme), rest = RestState(resting = true, alert = ALERT))
            shot("rest-edge", theme, desk(), clock(Face.Digits, theme), rest = RestState(resting = true), now = EDGE)
            shot("rest-edge-overview", theme, desk(), clock(Face.Overview, theme), rest = RestState(resting = true), now = EDGE)
            shot("rest-edge-calendar", theme, desk(), clock(Face.Calendar, theme), rest = RestState(resting = true), now = EDGE)
            shot("home-picking-rest", theme, desk(), clock(Face.Digits, theme, picking = true), restOpen = true)
            shot("home-ai-idle", theme, desk(), clock(theme = theme), ai = AiScreenState(sessions = emptyList(), busy = 0))
            shot("processes", theme, desk(Screen.Processes), clock(theme = theme))
        }
    }

    @Test
    fun largeText() {
        // 쉬는 화면과 쉬기 설정 판은 가장 낮은 접은 가로에서도 본다(검증 1차 5).
        if (device == "folded-landscape") {
            for (scale in listOf(1.3f, 2.0f)) {
                shot("font$scale-rest-edge", Theme.Dark, desk(), clock(Face.Overview, Theme.Dark), scale, rest = RestState(resting = true), now = EDGE)
                shot("font$scale-home-picking-rest", Theme.Dark, desk(), clock(Face.Digits, Theme.Dark, picking = true), scale, restOpen = true)
            }
        }
        if (!device.endsWith("portrait") || device.startsWith("unfolded")) return
        for (scale in listOf(1.3f, 2.0f)) {
            Face.entries.forEach { face ->
                shot("font${scale}-home-${face.name.lowercase()}", Theme.Dark, desk(), clock(face, Theme.Dark), scale)
            }
            shot("font$scale-perf", Theme.Dark, desk(Screen.Perf), clock(theme = Theme.Dark), scale)
            shot("font$scale-ai", Theme.Dark, desk(Screen.Ai), clock(theme = Theme.Dark), scale)
            shot("font$scale-rest-edge", Theme.Dark, desk(), clock(Face.Weather, Theme.Dark), scale, rest = RestState(resting = true), now = EDGE)
            shot("font$scale-home-picking-rest", Theme.Dark, desk(), clock(Face.Digits, Theme.Dark, picking = true), scale, restOpen = true)
            shot("font$scale-music", Theme.Dark, desk(Screen.Music), clock(theme = Theme.Dark), scale)
            shot("font$scale-keys-named", Theme.Dark, desk(Screen.Shortcuts), clock(theme = Theme.Dark), scale, keys = KEYS.copy(items = KEYS.items!!.map { it.copy(name = it.name.ifEmpty { "이름" }) }))
            shot("font$scale-home-ai-alert", Theme.Dark, desk(), clock(theme = Theme.Dark), scale, AI.copy(alert = ALERT))
            shot("font$scale-home-picking", Theme.Dark, desk(), clock(Face.Digits, Theme.Dark, picking = true), scale)
        }
    }
}
