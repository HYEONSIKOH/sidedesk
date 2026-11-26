package io.github.hyeonsikoh.sidedesk.rest

import io.github.hyeonsikoh.sidedesk.ai.AiScreenState
import io.github.hyeonsikoh.sidedesk.clock.FakeSettings
import io.github.hyeonsikoh.sidedesk.link.AiAlert
import io.github.hyeonsikoh.sidedesk.link.AlertKind
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.setMain
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class RestViewModelTest {
    private val main = StandardTestDispatcher()
    private val settings = FakeSettings()
    private val ai = MutableStateFlow(AiScreenState())
    private val five = RestAfter.Five.millis!!

    @Before
    fun setUp() = Dispatchers.setMain(main)

    @After
    fun tearDown() = Dispatchers.resetMain()

    private fun rest(): RestViewModel = RestViewModel(settings, ai).also {
        it.setVisible(true)
        main.scheduler.runCurrent()
    }

    private fun pass(millis: Long) {
        main.scheduler.advanceTimeBy(millis)
        main.scheduler.runCurrent()
    }

    private fun alert(id: Long) = AiAlert(id, "s", "문서 정리", AlertKind.Done, 0)

    @Test
    fun restsAfterFiveMinutesWithoutATouch() {
        val rest = rest()
        pass(five - 1)
        assertFalse(rest.state.value.resting)
        pass(2)
        assertTrue(rest.state.value.resting)
    }

    @Test
    fun aTouchStartsTheWaitAgainAndWakes() {
        val rest = rest()
        pass(five - 1_000)
        rest.touched()
        pass(five - 1_000)
        assertFalse(rest.state.value.resting)
        pass(2_000)
        assertTrue(rest.state.value.resting)
        rest.touched()
        assertFalse(rest.state.value.resting)
        pass(five + 1)
        assertTrue(rest.state.value.resting)
    }

    @Test
    fun neverRestsWhenSetToNever() {
        val rest = rest()
        rest.setRestAfter(RestAfter.Never)
        pass(60 * 60_000)
        assertFalse(rest.state.value.resting)
        assertEquals("Never", settings.restAfter)
    }

    @Test
    fun eachHoldStopsTheWaitAndStartsItAgainWhenLifted() {
        for (reason in Hold.entries) {
            val rest = rest()
            rest.hold(reason, true)
            pass(five * 2)
            assertFalse("$reason", rest.state.value.resting)
            rest.hold(reason, false)
            pass(five - 1)
            assertFalse("$reason", rest.state.value.resting)
            pass(2)
            assertTrue("$reason", rest.state.value.resting)
        }
    }

    @Test
    fun aHoldWakesTheRestingScreen() {
        val rest = rest()
        pass(five + 1)
        rest.hold(Hold.Pairing, true)
        assertFalse(rest.state.value.resting)
    }

    @Test
    fun hidingWakesAndStopsTheWait() {
        val rest = rest()
        pass(five + 1)
        rest.setVisible(false)
        assertFalse(rest.state.value.resting)
        pass(five * 2)
        assertFalse(rest.state.value.resting)
    }

    @Test
    fun showsAnAlertWhileRestingWithoutWaking() {
        val rest = rest()
        pass(five + 1)
        ai.value = AiScreenState(alert = alert(1))
        main.scheduler.runCurrent()
        assertTrue(rest.state.value.resting)
        assertEquals(1L, rest.state.value.alert!!.id)
        ai.value = AiScreenState(alert = null)
        main.scheduler.runCurrent()
        assertNull(rest.state.value.alert)
    }

    @Test
    fun wakesOnAFreshAlertWhenAskedAndWaitsAgain() {
        settings.wakeOnAlert = true
        val rest = rest()
        pass(five + 1)
        ai.value = AiScreenState(alert = alert(1))
        main.scheduler.runCurrent()
        assertFalse(rest.state.value.resting)
        pass(five + 1)
        assertTrue(rest.state.value.resting)
    }

    @Test
    fun aLateAlertDoesNotWake() {
        settings.wakeOnAlert = true
        val rest = rest()
        pass(five + 1)
        ai.value = AiScreenState(alert = alert(1), alertLate = true)
        main.scheduler.runCurrent()
        assertTrue(rest.state.value.resting)
        assertEquals(1L, rest.state.value.alert!!.id)
    }

    @Test
    fun anAlertWhileRestingBuzzesOnce() {
        val link = io.github.hyeonsikoh.sidedesk.link.FakeLink()
        var buzzes = 0
        val ai = io.github.hyeonsikoh.sidedesk.ai.AiViewModel(link, { buzzes += 1 }) { 0L }
        val rest = RestViewModel(settings, ai.state).also { it.setVisible(true) }
        main.scheduler.runCurrent()
        pass(five + 1)
        link.alerts.tryEmit(alert(1))
        main.scheduler.runCurrent()
        assertTrue(rest.state.value.resting)
        assertEquals(1L, rest.state.value.alert!!.id)
        assertEquals(1, buzzes)
    }

    @Test
    fun readsAndSavesTheSettings() {
        settings.keepOn = "Always"
        settings.vibration = "깨짐"
        val rest = rest()
        assertEquals(KeepOn.Always, rest.state.value.keepOn)
        assertEquals(Vibration.Normal, rest.state.value.vibration)
        assertEquals(RestAfter.Five, rest.state.value.restAfter)
        rest.setVibration(Vibration.Off)
        rest.setWakeOnAlert(true)
        rest.setKeepOn(KeepOn.Never)
        assertEquals("Off", settings.vibration)
        assertEquals(true, settings.wakeOnAlert)
        assertEquals("Never", settings.keepOn)
    }
}
