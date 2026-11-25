package io.github.hyeonsikoh.sidedesk.home

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.github.hyeonsikoh.sidedesk.link.ComputerLink
import io.github.hyeonsikoh.sidedesk.link.LinkState
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn

data class HomeState(val computerName: String? = null, val problem: String? = null)

class HomeViewModel(link: ComputerLink) : ViewModel() {
    val state: StateFlow<HomeState> = link.state
        .map(::toHome)
        .stateIn(viewModelScope, SharingStarted.Eagerly, toHome(link.state.value))

    private fun toHome(link: LinkState) = when (link) {
        LinkState.Waiting -> HomeState()
        is LinkState.Received -> HomeState(computerName = link.computerName)
        is LinkState.Failed -> HomeState(problem = link.reason)
    }
}
