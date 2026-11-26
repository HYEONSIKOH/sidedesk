package io.github.hyeonsikoh.sidedesk.clock

import io.github.hyeonsikoh.sidedesk.link.FakeLink
import io.github.hyeonsikoh.sidedesk.link.LinkState
import io.github.hyeonsikoh.sidedesk.link.Sky
import io.github.hyeonsikoh.sidedesk.link.Via
import io.github.hyeonsikoh.sidedesk.link.Weather
import io.github.hyeonsikoh.sidedesk.link.WeatherSource
import io.github.hyeonsikoh.sidedesk.ui.Theme
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.UnconfinedTestDispatcher
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
class ClockViewModelTest {
    private class FakeSettings(
        override var face: String? = null,
        override var theme: String? = null,
        override var askedLocation: Boolean = false,
    ) : Settings

    private val main = UnconfinedTestDispatcher()
    private val link = FakeLink()
    private val settings = FakeSettings()
    private var now = 10_000_000L

    @Before
    fun setUp() = Dispatchers.setMain(main)

    @After
    fun tearDown() = Dispatchers.resetMain()

    private fun clock() = ClockViewModel(link, settings) { now }

    private fun weather(fetchedAt: Long = now, source: WeatherSource = WeatherSource.OpenMeteo) =
        Weather(12f, Sky.Clear, 15f, 6f, true, fetchedAt, source)

    @Test
    fun startsWithTheSavedFaceAndTheme() {
        settings.face = "Calendar"
        settings.theme = "Light"
        val state = clock().state.value
        assertEquals(Face.Calendar, state.face)
        assertEquals(Theme.Light, state.theme)
    }

    @Test
    fun startsWithDefaultsForUnknownValues() {
        settings.face = "Moon"
        settings.theme = "깨짐"
        val state = clock().state.value
        assertEquals(Face.Digits, state.face)
        assertEquals(Theme.Dark, state.theme)
    }

    @Test
    fun swipingChangesTheFaceOnlyWhilePicking() {
        val clock = clock()
        clock.nextFace()
        assertEquals(Face.Digits, clock.state.value.face)
        clock.pick()
        clock.nextFace()
        assertEquals(Face.Dial, clock.state.value.face)
        clock.previousFace()
        clock.previousFace()
        assertEquals(Face.Overview, clock.state.value.face)
        assertEquals("Overview", settings.face)
        clock.done()
        assertFalse(clock.state.value.picking)
    }

    @Test
    fun savesTheTheme() {
        val clock = clock()
        clock.setTheme(Theme.Light)
        assertEquals(Theme.Light, clock.state.value.theme)
        assertEquals("Light", settings.theme)
    }

    @Test
    fun showsTheWayItIsConnected() {
        val clock = clock()
        assertNull(clock.state.value.via)
        link.state.value = LinkState.Connected("맥", Via.Usb)
        assertEquals(Via.Usb, clock.state.value.via)
        link.state.value = LinkState.Connected("맥", Via.Bluetooth)
        assertEquals(Via.Bluetooth, clock.state.value.via)
        link.state.value = LinkState.Lost("맥")
        assertNull(clock.state.value.via)
    }

    @Test
    fun hasNoWeatherAndNoCreditBeforeTheFirst() {
        val state = clock().state.value
        assertNull(state.weather)
        assertNull(state.credit)
        assertNull(state.shortCredit)
    }

    @Test
    fun dimsTheWeatherWhenLost() {
        val clock = clock()
        link.state.value = LinkState.Connected("맥")
        link.weather.value = weather()
        assertFalse(clock.state.value.weatherDimmed)
        link.state.value = LinkState.Lost("맥")
        assertTrue(clock.state.value.weatherDimmed)
        assertEquals(12f, clock.state.value.weather!!.temperature)
    }

    @Test
    fun dimsWeatherOlderThanAnHour() {
        val clock = clock()
        link.state.value = LinkState.Connected("맥")
        link.weather.value = weather(fetchedAt = now)
        now += ClockViewModel.STALE_MILLIS + 1
        main.scheduler.advanceTimeBy(60_001)
        assertTrue(clock.state.value.weatherDimmed)
    }

    @Test
    fun treatsWeatherFromTheFutureAsFresh() {
        val clock = clock()
        link.state.value = LinkState.Connected("맥")
        link.weather.value = weather(fetchedAt = now + 3_600_000)
        assertFalse(clock.state.value.weatherDimmed)
    }

    @Test
    fun creditsWhereTheWeatherCameFrom() {
        val clock = clock()
        link.weather.value = weather(source = WeatherSource.Kma)
        assertEquals("날씨 자료 기상청", clock.state.value.credit)
        assertEquals("기상청", clock.state.value.shortCredit)
        link.weather.value = weather(source = WeatherSource.OpenMeteo)
        assertEquals("날씨 자료 Open-Meteo.com", clock.state.value.credit)
        assertEquals("Open-Meteo", clock.state.value.shortCredit)
        link.weather.value = weather(source = WeatherSource.Unknown)
        assertNull(clock.state.value.credit)
    }
}
