package io.github.hyeonsikoh.sidedesk.ai

import io.github.hyeonsikoh.sidedesk.link.AiAlert
import io.github.hyeonsikoh.sidedesk.link.AiSession
import io.github.hyeonsikoh.sidedesk.link.AiState
import io.github.hyeonsikoh.sidedesk.link.AiTool
import io.github.hyeonsikoh.sidedesk.link.AlertKind
import io.github.hyeonsikoh.sidedesk.link.FakeLink
import io.github.hyeonsikoh.sidedesk.link.Forge
import io.github.hyeonsikoh.sidedesk.link.ForgeCounts
import io.github.hyeonsikoh.sidedesk.link.ForgeState
import io.github.hyeonsikoh.sidedesk.link.LinkState
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
class AiViewModelTest {
    private val main = UnconfinedTestDispatcher()
    private val link = FakeLink()
    private val buzzes = mutableListOf<AlertKind>()
    private var now = 100_000_000L

    @Before
    fun setUp() = Dispatchers.setMain(main)

    @After
    fun tearDown() = Dispatchers.resetMain()

    private fun ai() = AiViewModel(link, { buzzes += it }) { now }

    private fun session(id: String, state: AiState, since: Long) = AiSession(id, AiTool.ClaudeCode, id, "p", state, since, true)

    private fun alert(id: Long, kind: AlertKind = AlertKind.Done, at: Long = now) = AiAlert(id, "a", "로그인 화면", kind, at)

    @Test
    fun putsBusySessionsFirstThenTheRecent() {
        val ai = ai()
        link.sessions.value = listOf(
            session("done-old", AiState.Done, 1),
            session("work", AiState.Working, 5),
            session("unknown", AiState.Unknown, 9),
            session("wait", AiState.Waiting, 7),
            session("done-new", AiState.Done, 8),
        )
        assertEquals(listOf("wait", "work", "unknown", "done-new", "done-old"), ai.state.value.sessions.map { it.id })
    }

    @Test
    fun countsWorkingAndWaitingOnly() {
        val ai = ai()
        assertNull(ai.state.value.busy)
        link.sessions.value = listOf(
            session("a", AiState.Working, 1),
            session("b", AiState.Waiting, 1),
            session("c", AiState.Done, 1),
            session("d", AiState.Unknown, 1),
        )
        assertEquals(2, ai.state.value.busy)
        assertTrue(ai.state.value.waiting)
        link.sessions.value = emptyList()
        assertEquals(0, ai.state.value.busy)
        assertFalse(ai.state.value.waiting)
    }

    @Test
    fun switchesTheForgeTab() {
        val ai = ai()
        val github = ForgeCounts(ForgeState.Ok, 1, 2, 3)
        val gitlab = ForgeCounts(ForgeState.SignedOut, 0, 0, 0)
        link.forge.value = Forge(github, gitlab)
        assertEquals(github, ai.state.value.counts)
        ai.selectTab(ForgeTab.GitLab)
        assertEquals(gitlab, ai.state.value.counts)
    }

    @Test
    fun dimsWhileNotConnected() {
        val ai = ai()
        assertTrue(ai.state.value.dimmed)
        link.state.value = LinkState.Connected("맥")
        assertFalse(ai.state.value.dimmed)
    }

    @Test
    fun buzzesAndShowsANewAlertForFourSeconds() {
        val ai = ai()
        link.alerts.tryEmit(alert(1, AlertKind.Waiting))
        assertEquals(listOf(AlertKind.Waiting), buzzes)
        assertEquals(1L, ai.state.value.alert!!.id)
        main.scheduler.advanceTimeBy(AiViewModel.SHOW_MILLIS - 1)
        assertEquals(1L, ai.state.value.alert!!.id)
        main.scheduler.advanceTimeBy(2)
        assertNull(ai.state.value.alert)
    }

    @Test
    fun showsALateAlertWithoutBuzzing() {
        val ai = ai()
        link.alerts.tryEmit(alert(1, at = now - AiViewModel.LATE_MILLIS - 1))
        assertTrue(buzzes.isEmpty())
        assertEquals(1L, ai.state.value.alert!!.id)
    }

    @Test
    fun buzzesOnceForAlertsCloseTogether() {
        val ai = ai()
        link.alerts.tryEmit(alert(1))
        now += 1_000
        link.alerts.tryEmit(alert(2))
        assertEquals(1, buzzes.size)
        assertEquals(2L, ai.state.value.alert!!.id)
        now += AiViewModel.QUIET_MILLIS
        link.alerts.tryEmit(alert(3))
        assertEquals(2, buzzes.size)
    }

    @Test
    fun aNewerAlertKeepsShowingAfterTheOlderTimesOut() {
        val ai = ai()
        link.alerts.tryEmit(alert(1))
        main.scheduler.advanceTimeBy(3_000)
        link.alerts.tryEmit(alert(2))
        main.scheduler.advanceTimeBy(1_500)
        assertEquals(2L, ai.state.value.alert!!.id)
    }

    @Test
    fun writesElapsedTime() {
        val minute = 60_000L
        assertEquals("지금", elapsedText(now, now - 59_000))
        assertEquals("지금", elapsedText(now, now + 5 * minute))
        assertEquals("12분", elapsedText(now, now - 12 * minute))
        assertEquals("7시간 4분", elapsedText(now, now - (7 * 60 + 4) * minute))
        assertEquals("2시간", elapsedText(now, now - 120 * minute))
        assertEquals("1일 16시간", elapsedText(now, now - 40 * 60 * minute))
        assertEquals("3일", elapsedText(now, now - 72 * 60 * minute))
    }
}
