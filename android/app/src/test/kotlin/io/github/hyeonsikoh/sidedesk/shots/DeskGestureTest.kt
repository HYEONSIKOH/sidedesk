package io.github.hyeonsikoh.sidedesk.shots

import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.platform.UriHandler
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.longClick
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipeLeft
import androidx.compose.ui.test.swipeRight
import io.github.hyeonsikoh.sidedesk.clock.ClockState
import io.github.hyeonsikoh.sidedesk.clock.Face
import io.github.hyeonsikoh.sidedesk.clock.PickActions
import io.github.hyeonsikoh.sidedesk.desk.BIG
import io.github.hyeonsikoh.sidedesk.desk.ClockPart
import io.github.hyeonsikoh.sidedesk.desk.DeskActions
import io.github.hyeonsikoh.sidedesk.desk.DeskScreen
import io.github.hyeonsikoh.sidedesk.desk.DeskState
import io.github.hyeonsikoh.sidedesk.desk.Screen
import io.github.hyeonsikoh.sidedesk.ui.SideDeskTheme
import io.github.hyeonsikoh.sidedesk.ui.Theme
import org.junit.Assert.assertEquals
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.rules.RuleChain
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import java.util.TimeZone

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34], qualifiers = "w914dp-h411dp")
class DeskGestureTest {
    private val compose = createComposeRule()

    @get:Rule
    val rules: RuleChain = RuleChain.outerRule(registerActivity).around(compose)

    private val calls = mutableListOf<String>()
    private val opened = mutableListOf<String>()

    @Before
    fun setUp() {
        TimeZone.setDefault(TimeZone.getTimeZone("Asia/Seoul"))
        compose.mainClock.autoAdvance = false
    }

    private fun show(clock: ClockState, big: Screen = Screen.Home, dimmed: Boolean = false) {
        val actions = DeskActions(
            show = { calls += "show $it" },
            next = { calls += "next" },
            previous = { calls += "previous" },
            sortBy = {}, ask = {}, cancel = {}, confirmKill = {}, dismissNotice = {},
        )
        val part = ClockPart(
            clock,
            { DeskShots.NOW },
            pick = { calls += "pick" },
            nextFace = { calls += "nextFace" },
            previousFace = { calls += "previousFace" },
            actions = PickActions({ calls += "face $it" }, {}, { calls += "done" }),
        )
        val links = object : UriHandler {
            override fun openUri(uri: String) {
                opened += uri
            }
        }
        compose.setContent {
            CompositionLocalProvider(LocalUriHandler provides links) {
                SideDeskTheme(Theme.Dark) { DeskScreen(DeskState(big = big, dimmed = dimmed), part, actions) }
            }
        }
        compose.mainClock.advanceTimeBy(100)
    }

    private fun settle() = compose.mainClock.advanceTimeBy(1_000)

    @Test
    fun longPressOpensThePicker() {
        show(ClockState())
        compose.onNodeWithTag(BIG).performTouchInput { longClick() }
        settle()
        assertEquals(listOf("pick"), calls)
    }

    @Test
    fun swipingChangesTheScreen() {
        show(ClockState())
        compose.onNodeWithTag(BIG).performTouchInput { swipeLeft() }
        settle()
        compose.onNodeWithTag(BIG).performTouchInput { swipeRight() }
        settle()
        assertEquals(listOf("next", "previous"), calls)
    }

    @Test
    fun swipingWhilePickingChangesTheFace() {
        show(ClockState(picking = true))
        compose.onNodeWithTag(BIG).performTouchInput { swipeLeft() }
        settle()
        assertEquals(listOf("nextFace"), calls)
    }

    @Test
    fun tappingASmallTileWhilePickingClosesThePicker() {
        show(ClockState(picking = true))
        compose.onNodeWithText("CPU").performClick()
        settle()
        assertEquals(listOf("done", "show Perf"), calls)
    }

    @Test
    fun tappingTheCreditOpensTheLinkWithoutPicking() {
        show(ClockState(weather = DeskShots.SUNNY))
        compose.onNodeWithText("날씨 자료 Open-Meteo.com").performClick()
        settle()
        assertEquals(listOf("https://open-meteo.com/"), opened)
        assertEquals(emptyList<String>(), calls)
    }

    @Test
    fun drawsTheGivenTimeWhileLost() {
        show(ClockState(face = Face.Digits, weather = DeskShots.SUNNY, weatherDimmed = true), dimmed = true)
        compose.onNodeWithText("10:26").assertExists()
        compose.onNodeWithText("11월 25일 수요일").assertExists()
    }
}
