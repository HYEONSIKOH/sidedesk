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
    private var elapsed = 0L
    private val owner = LinkOwner(nanos = { now }, elapsed = { elapsed })
    private val mac = byteArrayOf(1)
    private val other = byteArrayOf(2)
    private val build = KillCommand(7, 10, 100)
    private val results = mutableListOf<CommandResult>()
    private val scope = CoroutineScope(Dispatchers.Unconfined).also {
        it.launch(start = CoroutineStart.UNDISPATCHED) { owner.results.collect { result -> results += result } }
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
        owner.send(build)
        assertEquals(listOf(build), owner.pendingCommands(mac))
        owner.receivedResult(CommandResult(7, true))
        assertEquals(listOf(CommandResult(7, true)), results)
        assertTrue(owner.pendingCommands(mac).isEmpty())
    }

    @Test
    fun dropsResultsOfUnknownCommands() {
        connect(mac)
        owner.receivedResult(CommandResult(99, true))
        owner.send(build)
        owner.receivedResult(CommandResult(7, true))
        owner.receivedResult(CommandResult(7, true))
        assertEquals(listOf(CommandResult(7, true)), results)
    }

    @Test
    fun sendsOnlyToTheComputerItWasChosenFor() {
        val mine = connect(mac)
        owner.send(build)
        owner.release(mine)
        connect(other)
        assertTrue(owner.pendingCommands(other).isEmpty())
        assertEquals(listOf(build), owner.pendingCommands(mac))
    }

    @Test
    fun keepsTheComputerAfterTheConnectionEnds() {
        owner.release(connect(mac))
        owner.send(build)
        assertEquals(listOf(build), owner.pendingCommands(mac))
        assertTrue(results.isEmpty())
    }

    @Test
    fun stopsSendingAfterTheLimit() {
        connect(mac)
        owner.send(build)
        now = LinkOwner.KILL_LIMIT_NANOS + 1
        assertTrue(owner.pendingCommands(mac).isEmpty())
    }

    @Test
    fun givingUpRemovesAndReportsOnce() {
        connect(mac)
        owner.send(build)
        owner.giveUp(7)
        owner.giveUp(7)
        owner.receivedResult(CommandResult(7, true))
        assertEquals(listOf(CommandResult(7, false, LinkOwner.NOT_CONNECTED)), results)
        assertTrue(owner.pendingCommands(mac).isEmpty())
    }

    @Test
    fun failsAtOnceWithoutAnyComputer() {
        owner.send(build)
        assertEquals(listOf(CommandResult(7, false, LinkOwner.NOT_CONNECTED)), results)
        assertTrue(owner.pendingCommands(mac).isEmpty())
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

    private fun alert(id: Long) = AiAlert(id, "s", "로그인 화면", AlertKind.Done, 1)

    private val received = mutableListOf<AiAlert>()
    private val alertScope = CoroutineScope(Dispatchers.Unconfined).also {
        it.launch(start = CoroutineStart.UNDISPATCHED) { owner.alerts.collect { a -> received += a } }
    }

    @Test
    fun takesSessionsAndForgeOnlyFromTheCurrentConnection() {
        val old = connect(mac)
        val mine = connect(mac)
        val list = listOf(AiSession("a", AiTool.ClaudeCode, "t", "p", AiState.Working, 1, true))
        val counts = ForgeCounts(ForgeState.Ok, 1, 2, 3)
        owner.receivedSessions(old, list)
        owner.receivedForge(old, Forge(counts, counts))
        assertEquals(null, owner.sessions.value)
        assertEquals(null, owner.forge.value)
        owner.receivedSessions(mine, list)
        owner.receivedForge(mine, Forge(counts, counts))
        assertEquals(list, owner.sessions.value)
        assertEquals(counts, owner.forge.value!!.github)
    }

    @Test
    fun passesEachAlertOncePerConnection() {
        val mine = connect(mac)
        owner.receivedAlert(mine, alert(1))
        owner.receivedAlert(mine, alert(1))
        owner.receivedAlert(mine, alert(2))
        assertEquals(listOf(1L, 2L), received.map { it.id })
        alertScope.cancel()
    }

    @Test
    fun takesTheSameNumberAgainAfterReconnecting() {
        val first = connect(mac)
        owner.receivedAlert(first, alert(1))
        val second = connect(mac)
        owner.receivedAlert(first, alert(5))
        owner.receivedAlert(second, alert(1))
        assertEquals(listOf(1L, 1L), received.map { it.id })
        alertScope.cancel()
    }

    private val terminal = Shortcut(3, "터미널", "terminal", "slate", Toggle.None, false)

    @Test
    fun runsOnlyShortcutsFromTheConnectedComputer() {
        val mine = connect(mac)
        owner.receivedShortcuts(mine, 42, listOf(terminal))
        assertTrue(owner.runShortcut(RunShortcut(1, 3)))
        assertEquals(listOf<Command>(RunShortcut(1, 3, 42)), owner.pendingCommands(mac))
        assertEquals(false, owner.runShortcut(RunShortcut(2, 99)))
    }

    @Test
    fun forgetsTheListWhenAnotherComputerConnects() {
        owner.receivedShortcuts(connect(mac), 42, listOf(terminal))
        val mine = owner.takeUsb {}
        assertEquals(listOf(terminal), owner.shortcuts.value)
        owner.ready(mine, other)
        assertEquals(null, owner.shortcuts.value)
        assertEquals(false, owner.runShortcut(RunShortcut(1, 3)))
        assertTrue(owner.pendingCommands(other).isEmpty())
    }

    @Test
    fun keepsTheListWhenTheSameComputerComesBack() {
        owner.receivedShortcuts(connect(mac), 42, listOf(terminal))
        connect(mac)
        assertEquals(listOf(terminal), owner.shortcuts.value)
        assertTrue(owner.runShortcut(RunShortcut(1, 3)))
    }

    @Test
    fun doesNotRunWhileNotConnected() {
        val mine = connect(mac)
        owner.receivedShortcuts(mine, 42, listOf(terminal))
        owner.release(mine)
        assertEquals(false, owner.runShortcut(RunShortcut(1, 3)))
    }

    private val song = NowPlaying(MusicPlayer.Music, "여름 바다", "가수", "앨범", 221_000, 5, true, 84_000, true, true, true)

    @Test
    fun stampsNowPlayingWithThePhoneClock() {
        val mine = connect(mac)
        // 받은 때는 깊은 잠도 세는 부팅 뒤 시각으로 적는다(단조 nanos 가 아님).
        now = 1
        elapsed = 7_000
        owner.receivedNowPlaying(mine, song)
        assertEquals(7_000L, owner.nowPlaying.value!!.receivedAt)
    }

    @Test
    fun sendsPlaybackCommandsOnlyForTheComputerShowingTheSong() {
        owner.receivedNowPlaying(connect(mac), song)
        assertTrue(owner.music(MusicCommand(1, MusicAction.PlayPause)))
        assertEquals(listOf<Command>(MusicCommand(1, MusicAction.PlayPause)), owner.pendingCommands(mac))
        val usb = owner.takeUsb {}
        assertEquals(false, owner.music(MusicCommand(2, MusicAction.Next)))
        owner.ready(usb, other)
        assertEquals(null, owner.nowPlaying.value)
        assertEquals(false, owner.music(MusicCommand(3, MusicAction.Next)))
    }

    @Test
    fun keepsOnlyTheLastVolumeForThisConnection() {
        val mine = connect(mac)
        owner.receivedVolume(mine, Volume(true, 40, false, true))
        assertTrue(owner.music(MusicCommand(1, MusicAction.SetVolume, 50)))
        assertTrue(owner.music(MusicCommand(2, MusicAction.SetVolume, 60)))
        assertTrue(owner.pendingCommands(mac).isEmpty())
        assertEquals(MusicCommand(2, MusicAction.SetVolume, 60), owner.takeVolume(mine))
        assertEquals(null, owner.takeVolume(mine))
    }

    @Test
    fun dropsAVolumeHeldForAnOldConnection() {
        val first = connect(mac)
        owner.receivedVolume(first, Volume(true, 40, false, true))
        owner.music(MusicCommand(1, MusicAction.SetVolume, 50))
        val second = connect(mac)
        assertEquals(null, owner.takeVolume(second))
    }

    @Test
    fun passesVolumeFailures() {
        val mine = connect(mac)
        owner.receivedVolume(mine, Volume(true, 40, false, true))
        owner.music(MusicCommand(9, MusicAction.SetVolume, 50))
        owner.receivedResult(CommandResult(9, false, "볼륨을 바꿀 수 없는 장치입니다"))
        assertEquals(listOf(CommandResult(9, false, "볼륨을 바꿀 수 없는 장치입니다")), results)
    }

    @Test
    fun keepsTheLastEightArtworks() {
        val mine = connect(mac)
        (1L..9L).forEach { owner.receivedArtwork(mine, it, byteArrayOf(it.toByte())) }
        assertEquals((2L..9L).toList(), owner.artworks.value.keys.toList())
        owner.receivedArtwork(mine, 0, byteArrayOf(1))
        assertEquals(8, owner.artworks.value.size)
    }
}
