package io.github.hyeonsikoh.sidedesk.home

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.github.hyeonsikoh.sidedesk.link.ComputerLink
import io.github.hyeonsikoh.sidedesk.link.LinkState
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.stateIn

data class HomeState(
    val computerName: String? = null,
    /** 끊겨서 마지막 값을 흐리게 보인다 */
    val dimmed: Boolean = false,
    val pairingCode: String? = null,
    val problem: String? = null,
    /** 블루투스를 쓸 수 없는 이유. USB 는 된다. */
    val notice: String? = null,
)

class HomeViewModel(private val link: ComputerLink) : ViewModel() {
    val state: StateFlow<HomeState> = combine(link.state, link.bluetoothProblem) { state, problem ->
        toHome(state).copy(notice = problem)
    }.stateIn(
        viewModelScope,
        SharingStarted.Eagerly,
        toHome(link.state.value).copy(notice = link.bluetoothProblem.value),
    )

    fun answer(same: Boolean) = link.answer(same)

    private fun toHome(link: LinkState) = when (link) {
        LinkState.Waiting -> HomeState()
        is LinkState.Pairing -> HomeState(pairingCode = link.code)
        is LinkState.Connected -> HomeState(computerName = link.computerName)
        is LinkState.Lost -> HomeState(computerName = link.computerName, dimmed = true)
        is LinkState.Failed -> HomeState(problem = link.reason)
    }
}
