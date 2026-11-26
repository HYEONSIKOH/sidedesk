package io.github.hyeonsikoh.sidedesk.shortcut

import io.github.hyeonsikoh.sidedesk.ai.Buzzer
import io.github.hyeonsikoh.sidedesk.link.CommandResult
import io.github.hyeonsikoh.sidedesk.link.FakeLink
import io.github.hyeonsikoh.sidedesk.link.LinkOwner
import io.github.hyeonsikoh.sidedesk.link.LinkState
import io.github.hyeonsikoh.sidedesk.link.RunShortcut
import io.github.hyeonsikoh.sidedesk.link.Shortcut
import io.github.hyeonsikoh.sidedesk.link.Toggle
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
class ShortcutViewModelTest {
    private val main = UnconfinedTestDispatcher()
    private val link = FakeLink()
    private var taps = 0
    private var next = 100L
    private val buzzer = object : Buzzer {
        override fun buzz(kind: io.github.hyeonsikoh.sidedesk.link.AlertKind) = Unit

        override fun tap() {
            taps += 1
        }
    }
    private val terminal = Shortcut(3, "터미널", "terminal", "slate", Toggle.None, false)
    private val blank = Shortcut(4, "", "star", "gray", Toggle.None, false)

    @Before
    fun setUp() {
        Dispatchers.setMain(main)
        link.state.value = LinkState.Connected("맥")
        link.shortcuts.value = listOf(terminal, blank)
    }

    @After
    fun tearDown() = Dispatchers.resetMain()

    private fun vm() = ShortcutViewModel(link, buzzer) { next++ }

    @Test
    fun pressingBuzzesAndSendsOnlyTheNumber() {
        vm().press(3)
        assertEquals(1, taps)
        assertEquals(listOf(RunShortcut(100, 3)), link.kills)
    }

    @Test
    fun successIsSilent() {
        val vm = vm()
        vm.press(3)
        link.results.tryEmit(CommandResult(100, true, ""))
        assertNull(vm.state.value.notice)
    }

    @Test
    fun failureNamesTheShortcutForFourSeconds() {
        val vm = vm()
        vm.press(3)
        link.results.tryEmit(CommandResult(100, false, "앱을 찾지 못했습니다"))
        assertEquals("터미널을 실행하지 못했습니다. 앱을 찾지 못했습니다", vm.state.value.notice)
        main.scheduler.advanceTimeBy(ShortcutViewModel.NOTICE_MILLIS + 1)
        assertNull(vm.state.value.notice)
    }

    @Test
    fun doesNotRepeatTheGenericReason() {
        val vm = vm()
        vm.press(3)
        link.results.tryEmit(CommandResult(100, false, "실행하지 못했습니다"))
        assertEquals("터미널을 실행하지 못했습니다", vm.state.value.notice)
    }

    @Test
    fun aBlankNameSaysShortcut() {
        val vm = vm()
        vm.press(4)
        link.results.tryEmit(CommandResult(100, false, ""))
        assertEquals("단축키를 실행하지 못했습니다", vm.state.value.notice)
    }

    @Test
    fun givesUpAfterTheLimit() {
        vm().press(3)
        main.scheduler.advanceTimeBy(LinkOwner.KILL_LIMIT_MILLIS + 1)
        assertEquals(listOf(100L), link.givenUp)
    }

    @Test
    fun saysNotConnectedWhenTheLinkRefuses() {
        link.acceptShortcuts = false
        val vm = vm()
        vm.press(3)
        assertEquals(LinkOwner.NOT_CONNECTED, vm.state.value.notice)
        assertTrue(link.kills.isEmpty())
    }

    @Test
    fun ignoresNumbersNotInTheList() {
        val vm = vm()
        vm.press(99)
        assertEquals(0, taps)
        assertTrue(link.kills.isEmpty())
        assertNull(vm.state.value.notice)
    }

    @Test
    fun ignoresResultsOfOtherCommands() {
        val vm = vm()
        link.results.tryEmit(CommandResult(7, false, "x"))
        assertNull(vm.state.value.notice)
    }

    @Test
    fun dimsWhileNotConnected() {
        val vm = vm()
        link.state.value = LinkState.Lost("맥")
        assertTrue(vm.state.value.dimmed)
    }

    @Test
    fun laysOutTheGrid() {
        assertEquals(Grid(4, 3, (800f - 36f) / 4, false).columns, gridOf(800f, 400f, 11).columns)
        val wide = gridOf(800f, 400f, 11)
        assertEquals(3, wide.rows)
        assertEquals(minOf((800f - 36f) / 4, (400f - 24f) / 3), wide.cell)
        val tall = gridOf(400f, 800f, 11)
        assertEquals(3, tall.columns)
        assertEquals(4, tall.rows)
        val many = gridOf(800f, 400f, 14)
        assertEquals(4, many.rows)
        assertTrue(many.scrolls)
    }
}
