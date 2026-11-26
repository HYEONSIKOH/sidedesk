package io.github.hyeonsikoh.sidedesk.link

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class LinkOwnerTest {
    private var now = 0L
    private val owner = LinkOwner { now }
    private val mac = byteArrayOf(1)
    private val other = byteArrayOf(2)
    private val build = KillCommand(7, 10, 100)
    private val results = mutableListOf<KillResult>()
    private val scope = CoroutineScope(Dispatchers.Unconfined).also {
        it.launch(start = CoroutineStart.UNDISPATCHED) { owner.killResults.collect { result -> results += result } }
    }

    @After
    fun tearDown() = scope.cancel()

    private fun connect(computer: ByteArray): Int {
        val mine = owner.takeUsb {}
        owner.ready(mine, computer)
        return mine
    }

    @Test
    fun keepsACommandUntilItsResultComes() {
        connect(mac)
        owner.kill(build)
        assertEquals(listOf(build), owner.pendingKills(mac))
        owner.receivedKill(KillResult(7, true))
        assertEquals(listOf(KillResult(7, true)), results)
        assertTrue(owner.pendingKills(mac).isEmpty())
    }

    @Test
    fun dropsResultsOfUnknownCommands() {
        connect(mac)
        owner.receivedKill(KillResult(99, true))
        owner.kill(build)
        owner.receivedKill(KillResult(7, true))
        owner.receivedKill(KillResult(7, true))
        assertEquals(listOf(KillResult(7, true)), results)
    }

    @Test
    fun sendsOnlyToTheComputerItWasChosenFor() {
        val mine = connect(mac)
        owner.kill(build)
        owner.release(mine)
        connect(other)
        assertTrue(owner.pendingKills(other).isEmpty())
        assertEquals(listOf(build), owner.pendingKills(mac))
    }

    @Test
    fun keepsTheComputerAfterTheConnectionEnds() {
        owner.release(connect(mac))
        owner.kill(build)
        assertEquals(listOf(build), owner.pendingKills(mac))
        assertTrue(results.isEmpty())
    }

    @Test
    fun stopsSendingAfterTheLimit() {
        connect(mac)
        owner.kill(build)
        now = LinkOwner.KILL_LIMIT_NANOS + 1
        assertTrue(owner.pendingKills(mac).isEmpty())
    }

    @Test
    fun givingUpRemovesAndReportsOnce() {
        connect(mac)
        owner.kill(build)
        owner.giveUp(7)
        owner.giveUp(7)
        owner.receivedKill(KillResult(7, true))
        assertEquals(listOf(KillResult(7, false, LinkOwner.NOT_CONNECTED)), results)
        assertTrue(owner.pendingKills(mac).isEmpty())
    }

    @Test
    fun failsAtOnceWithoutAnyComputer() {
        owner.kill(build)
        assertEquals(listOf(KillResult(7, false, LinkOwner.NOT_CONNECTED)), results)
        assertTrue(owner.pendingKills(mac).isEmpty())
    }

    private val sunny = Weather(12f, Sky.Clear, 15f, 6f, true, 1, WeatherSource.OpenMeteo)

    @Test
    fun takesWeatherOnlyFromTheCurrentConnection() {
        val old = connect(mac)
        val mine = connect(mac)
        owner.receivedWeather(old, sunny)
        assertEquals(null, owner.weather.value)
        owner.receivedWeather(mine, sunny)
        assertEquals(sunny, owner.weather.value)
    }

    @Test
    fun forgetsTheWeatherWhenLocationIsTurnedOff() {
        val mine = connect(mac)
        owner.receivedWeather(mine, sunny)
        owner.setLocation(LocationState.Off)
        assertEquals(null, owner.weather.value)
        owner.receivedWeather(mine, sunny)
        assertEquals(null, owner.weather.value)
        owner.setLocation(LocationState.At(37.5, 127.0))
        owner.receivedWeather(mine, sunny)
        assertEquals(sunny, owner.weather.value)
    }

    @Test
    fun startsWithUnknownLocation() {
        assertEquals(LocationState.Unknown, owner.location.get())
    }
}
