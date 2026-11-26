package io.github.hyeonsikoh.sidedesk.home

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import io.github.hyeonsikoh.sidedesk.link.FakeLink
import io.github.hyeonsikoh.sidedesk.link.LinkState
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.setMain
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Before
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class HomeViewModelTest {
    private val link = FakeLink()

    @Before
    fun setUp() = Dispatchers.setMain(UnconfinedTestDispatcher())

    @After
    fun tearDown() = Dispatchers.resetMain()

    @Test
    fun startsByWaiting() {
        assertEquals(HomeState(), HomeViewModel(link).state.value)
    }

    @Test
    fun showsTheLatestName() {
        val home = HomeViewModel(link)
        link.state.value = LinkState.Connected("a")
        link.state.value = LinkState.Connected("b")
        assertEquals(HomeState(computerName = "b"), home.state.value)
    }

    @Test
    fun showsAFailureAndRecovers() {
        val home = HomeViewModel(link)
        link.state.value = LinkState.Failed("열지 못함")
        assertEquals(HomeState(problem = "열지 못함"), home.state.value)
        link.state.value = LinkState.Waiting
        assertEquals(HomeState(), home.state.value)
    }

    @Test
    fun showsTheCodeAndPassesTheAnswer() {
        val home = HomeViewModel(link)
        link.state.value = LinkState.Pairing("123456")
        assertEquals(HomeState(pairingCode = "123456"), home.state.value)
        home.answer(true)
        assertEquals(listOf(true), link.answers)
    }

    @Test
    fun dimsTheLastNameWhenLost() {
        val home = HomeViewModel(link)
        link.state.value = LinkState.Connected("맥")
        link.state.value = LinkState.Lost("맥")
        assertEquals(HomeState(computerName = "맥", dimmed = true), home.state.value)
    }
}
