package io.github.hyeonsikoh.sidedesk.home

import io.github.hyeonsikoh.sidedesk.link.ComputerLink
import io.github.hyeonsikoh.sidedesk.link.LinkState
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.setMain
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Before
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class HomeViewModelTest {
    private class FakeLink : ComputerLink {
        override val state = MutableStateFlow<LinkState>(LinkState.Waiting)

        override fun start() = Unit

        override fun stop() = Unit
    }

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
        link.state.value = LinkState.Received("a")
        link.state.value = LinkState.Received("b")
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
}
