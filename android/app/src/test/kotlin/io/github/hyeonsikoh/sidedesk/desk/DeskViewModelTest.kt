package io.github.hyeonsikoh.sidedesk.desk

import io.github.hyeonsikoh.sidedesk.link.FakeLink
import io.github.hyeonsikoh.sidedesk.link.KillCommand
import io.github.hyeonsikoh.sidedesk.link.CommandResult
import io.github.hyeonsikoh.sidedesk.link.LinkOwner
import io.github.hyeonsikoh.sidedesk.link.LinkState
import io.github.hyeonsikoh.sidedesk.link.Perf
import io.github.hyeonsikoh.sidedesk.link.ProcessInfo
import io.github.hyeonsikoh.sidedesk.link.Processes
import io.github.hyeonsikoh.sidedesk.link.Subscription
import io.github.hyeonsikoh.sidedesk.link.Topic
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.setMain
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class DeskViewModelTest {
    private val link = FakeLink()
    private val build = ProcessInfo(10, 100, "빌드", 11.8f, 424_000_000)
    private val editor = ProcessInfo(20, 200, "편집기", 4.2f, 812_000_000)
    private val browser = ProcessInfo(30, 300, "브라우저", 2.1f, 1_900_000_000)

    private val main = UnconfinedTestDispatcher()

    @Before
    fun setUp() = Dispatchers.setMain(main)

    @After
    fun tearDown() = Dispatchers.resetMain()

    private fun perf(cpu: Float, rx: Long = 0, tx: Long = 0) = Perf(cpu, 12f, 58f, 44f, rx, tx)

    @Test
    fun showsTheLatestPerfAndKeepsNetworkHistory() {
        val desk = DeskViewModel(link) { 1L }
        link.perf.value = perf(10f, rx = 1, tx = 2)
        link.perf.value = perf(31f, rx = 3, tx = 4)
        val state = desk.state.value
        assertEquals(31f, state.perf!!.cpu)
        assertEquals(listOf(1L, 3L), state.received)
        assertEquals(listOf(2L, 4L), state.sent)
    }

    @Test
    fun keepsOnlyTheLastMinuteOfNetwork() {
        val desk = DeskViewModel(link) { 1L }
        repeat(DeskViewModel.HISTORY + 5) { link.perf.value = perf(it.toFloat(), rx = it.toLong()) }
        val received = desk.state.value.received
        assertEquals(DeskViewModel.HISTORY, received.size)
        assertEquals((DeskViewModel.HISTORY + 4).toLong(), received.last())
    }

    @Test
    fun sortsProcessesByCpuThenMemory() {
        val desk = DeskViewModel(link) { 1L }
        link.processes.value = Processes(listOf(browser, build, editor))
        assertEquals(listOf(build, editor, browser), desk.state.value.rows)
        desk.sortBy(Sort.Memory)
        assertEquals(listOf(browser, editor, build), desk.state.value.rows)
        link.processes.value = Processes(listOf(build, editor))
        assertEquals(listOf(editor, build), desk.state.value.rows)
    }

    @Test
    fun subscribesToWhatIsVisibleAndClearsWhenHidden() {
        val desk = DeskViewModel(link) { 1L }
        desk.setVisible(true)
        desk.show(Screen.Perf)
        desk.show(Screen.Processes)
        desk.setVisible(false)
        assertEquals(
            listOf(
                Subscription(Topic.Weather, setOf(Topic.Ai, Topic.Perf, Topic.Processes, Topic.Shortcuts, Topic.Music)),
                Subscription(Topic.Perf, setOf(Topic.Weather, Topic.Ai, Topic.Processes, Topic.Shortcuts, Topic.Music)),
                Subscription(Topic.Processes, setOf(Topic.Weather, Topic.Ai, Topic.Perf, Topic.Shortcuts, Topic.Music)),
                Subscription.NONE,
            ),
            link.subscriptions,
        )
    }

    @Test
    fun startsAtHome() {
        val desk = DeskViewModel(link) { 1L }
        assertEquals(Screen.Home, desk.state.value.big)
        assertEquals(listOf(Screen.Ai, Screen.Perf, Screen.Processes, Screen.Shortcuts, Screen.Music), desk.state.value.small)
    }

    @Test
    fun swipingGoesAroundTheScreens() {
        val desk = DeskViewModel(link) { 1L }
        desk.next()
        assertEquals(Screen.Ai, desk.state.value.big)
        desk.next()
        assertEquals(Screen.Perf, desk.state.value.big)
        desk.next()
        assertEquals(Screen.Processes, desk.state.value.big)
        desk.next()
        assertEquals(Screen.Shortcuts, desk.state.value.big)
        desk.next()
        assertEquals(Screen.Music, desk.state.value.big)
        desk.next()
        assertEquals(Screen.Home, desk.state.value.big)
        desk.previous()
        assertEquals(Screen.Music, desk.state.value.big)
    }

    @Test
    fun staysPutWhileAskingToKill() {
        val desk = DeskViewModel(link) { 1L }
        desk.show(Screen.Processes)
        desk.ask(build)
        desk.next()
        desk.show(Screen.Home)
        assertEquals(Screen.Processes, desk.state.value.big)
        desk.cancel()
        desk.show(Screen.Home)
        assertEquals(Screen.Home, desk.state.value.big)
    }

    @Test
    fun doesNotSubscribeWhileHidden() {
        val desk = DeskViewModel(link) { 1L }
        desk.show(Screen.Processes)
        assertEquals(listOf(Subscription.NONE), link.subscriptions)
    }

    @Test
    fun cancelingSendsNothing() {
        val desk = DeskViewModel(link) { 1L }
        desk.ask(build)
        assertEquals(build, desk.state.value.asking)
        desk.cancel()
        assertNull(desk.state.value.asking)
        assertTrue(link.kills.isEmpty())
    }

    @Test
    fun confirmingSendsANumberedCommandAndShowsTheResult() {
        var next = 1_000L
        val desk = DeskViewModel(link) { next++ }
        desk.ask(build)
        desk.confirmKill()
        desk.ask(editor)
        desk.confirmKill()
        assertEquals(listOf(KillCommand(1_000, 10, 100), KillCommand(1_001, 20, 200)), link.kills)
        assertNull(desk.state.value.asking)

        link.results.tryEmit(CommandResult(1_000, true))
        assertEquals("빌드를 끝냈습니다", desk.state.value.notice)
        link.results.tryEmit(CommandResult(1_001, false, "이미 끝난 프로그램입니다"))
        assertEquals("편집기를 끝내지 못했습니다. 이미 끝난 프로그램입니다", desk.state.value.notice)
        desk.dismissNotice()
        assertNull(desk.state.value.notice)
    }

    @Test
    fun picksTheParticleByTheLastLetter() {
        var next = 1L
        val desk = DeskViewModel(link) { next++ }
        desk.ask(ProcessInfo(1, 1, "음악", 0f, 0))
        desk.confirmKill()
        link.results.tryEmit(CommandResult(1, false))
        assertEquals("음악을 끝내지 못했습니다", desk.state.value.notice)
        desk.ask(ProcessInfo(2, 2, "Safari", 0f, 0))
        desk.confirmKill()
        link.results.tryEmit(CommandResult(2, true))
        assertEquals("Safari을(를) 끝냈습니다", desk.state.value.notice)
    }

    @Test
    fun givesUpAfterTheLimit() {
        val desk = DeskViewModel(link) { 5L }
        desk.ask(build)
        desk.confirmKill()
        main.scheduler.advanceTimeBy(LinkOwner.KILL_LIMIT_MILLIS - 1)
        assertTrue(link.givenUp.isEmpty())
        main.scheduler.advanceTimeBy(2)
        assertEquals(listOf(5L), link.givenUp)
    }

    @Test
    fun confirmingWithoutAskingDoesNothing() {
        DeskViewModel(link) { 1L }.confirmKill()
        assertTrue(link.kills.isEmpty())
    }

    @Test
    fun ignoresResultsOfOtherCommands() {
        val desk = DeskViewModel(link) { 1L }
        link.results.tryEmit(CommandResult(7, true))
        assertNull(desk.state.value.notice)
    }

    @Test
    fun dimsAndKeepsTheLastValuesWhenLost() {
        val desk = DeskViewModel(link) { 1L }
        link.perf.value = perf(31f)
        link.state.value = LinkState.Lost("맥")
        assertTrue(desk.state.value.dimmed)
        assertEquals(31f, desk.state.value.perf!!.cpu)
        link.state.value = LinkState.Connected("맥")
        assertEquals(false, desk.state.value.dimmed)
    }

    @Test
    fun dimsLikeTheWeatherWhileNotConnected() {
        val desk = DeskViewModel(link) { 1L }
        link.state.value = LinkState.Connected("맥")
        assertEquals(false, desk.state.value.dimmed)
        link.state.value = LinkState.Waiting
        assertTrue(desk.state.value.dimmed)
    }

    @Test
    fun subscribesToTheAiScreen() {
        val desk = DeskViewModel(link) { 1L }
        desk.setVisible(true)
        desk.show(Screen.Ai)
        assertEquals(Subscription(Topic.Ai, setOf(Topic.Weather, Topic.Perf, Topic.Processes, Topic.Shortcuts, Topic.Music)), link.subscriptions.last())
        desk.show(Screen.Shortcuts)
        assertEquals(Subscription(Topic.Shortcuts, setOf(Topic.Weather, Topic.Ai, Topic.Perf, Topic.Processes, Topic.Music)), link.subscriptions.last())
    }

    @Test
    fun restingAsksOnlyForWeatherAndAi() {
        val desk = DeskViewModel(link) { 1L }
        desk.setVisible(true)
        desk.show(Screen.Perf)
        desk.setResting(true)
        assertEquals(Subscription(Topic.Weather, setOf(Topic.Ai)), link.subscriptions.last())
        desk.setResting(false)
        assertEquals(Subscription(Topic.Perf, setOf(Topic.Weather, Topic.Ai, Topic.Processes, Topic.Shortcuts, Topic.Music)), link.subscriptions.last())
    }

    @Test
    fun comingBackAfterRestingAsksForTheUsualTable() {
        val desk = DeskViewModel(link) { 1L }
        desk.setVisible(true)
        desk.setResting(true)
        // onStop 은 보이기와 쉼을 함께 푼다.
        desk.setVisible(false)
        desk.setResting(false)
        desk.setVisible(true)
        assertEquals(Subscription(Topic.Weather, setOf(Topic.Ai, Topic.Perf, Topic.Processes, Topic.Shortcuts, Topic.Music)), link.subscriptions.last())
    }
}
