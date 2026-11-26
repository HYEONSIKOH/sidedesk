package io.github.hyeonsikoh.sidedesk.shots

import androidx.activity.ComponentActivity
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import io.github.hyeonsikoh.sidedesk.clock.ClockState
import io.github.hyeonsikoh.sidedesk.clock.PickActions
import io.github.hyeonsikoh.sidedesk.desk.ClockPart
import io.github.hyeonsikoh.sidedesk.desk.DeskActions
import io.github.hyeonsikoh.sidedesk.desk.DeskScreen
import io.github.hyeonsikoh.sidedesk.desk.DeskState
import io.github.hyeonsikoh.sidedesk.desk.Screen
import io.github.hyeonsikoh.sidedesk.desk.ShortcutPart
import io.github.hyeonsikoh.sidedesk.rest.RestScreen
import io.github.hyeonsikoh.sidedesk.rest.RestState
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

/** 쉬는 판은 누름을 깨우기에만 쓰고 아래로 보내지 않는다. 깨어 있을 때 누름은 그대로 가며 손댐이 잡힌다. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34], qualifiers = "w914dp-h411dp")
class RestGestureTest {
    private val compose = createAndroidComposeRule<ComponentActivity>()

    @get:Rule
    val rules: RuleChain = RuleChain.outerRule(registerActivity).around(compose)

    private val calls = mutableListOf<String>()
    private var resting by mutableStateOf(false)

    @Before
    fun setUp() {
        compose.mainClock.autoAdvance = false
        val keys = DeskShots.KEYS.copy(items = DeskShots.KEYS.items!!.map { it.copy(name = "키 ${it.id}") })
        val actions = DeskActions({}, {}, {}, {}, {}, {}, {}, {})
        compose.setContent {
            SideDeskTheme(Theme.Dark) {
                Box(
                    Modifier.fillMaxSize().pointerInput(Unit) {
                        awaitEachGesture {
                            awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
                            calls += "touched"
                        }
                    },
                ) {
                    DeskScreen(
                        DeskState(big = Screen.Shortcuts),
                        ClockPart(ClockState(), { DeskShots.NOW }, {}, {}, {}, PickActions({}, {}, {})),
                        actions,
                        shortcuts = ShortcutPart(keys) { calls += "press $it" },
                    )
                    RestScreen(resting, RestState(resting = resting), ClockState(), DeskShots.NOW) { calls += "wake" }
                }
            }
        }
        compose.mainClock.advanceTimeBy(100)
    }

    private fun settle(ms: Long = 1_000) = compose.mainClock.advanceTimeBy(ms)

    @Test
    fun aTouchWhileAwakeGoesThrough() {
        compose.onNodeWithText("키 3").performClick()
        settle()
        assertEquals(listOf("touched", "press 3"), calls)
    }

    @Test
    fun aTouchWhileRestingOnlyWakes() {
        resting = true
        settle()
        compose.onNodeWithText("키 3", useUnmergedTree = true).performClick()
        settle()
        assertEquals(listOf("touched", "wake"), calls)
    }

    @Test
    fun aTouchWhileFadingOutStillOnlyWakes() {
        resting = true
        settle()
        resting = false
        settle(100)
        compose.onNodeWithText("키 3", useUnmergedTree = true).performClick()
        settle()
        assertEquals(false, calls.contains("press 3"))
    }

    @Test
    fun backWhileRestingOnlyWakes() {
        resting = true
        settle()
        compose.runOnUiThread { compose.activity.onBackPressedDispatcher.onBackPressed() }
        settle()
        assertEquals(listOf("wake"), calls)
        assertEquals(false, compose.activity.isFinishing)
    }
}
