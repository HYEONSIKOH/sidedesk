package io.github.hyeonsikoh.sidedesk.link

import kotlinx.coroutines.flow.StateFlow

sealed interface LinkState {
    data object Waiting : LinkState

    /** 컴퓨터 터미널에 뜬 숫자와 같은지 사람이 확인해야 한다. */
    data class Pairing(val code: String) : LinkState

    data class Connected(val computerName: String?) : LinkState

    /** 끊겼다. 마지막 이름을 흐리게 보인다. */
    data class Lost(val computerName: String?) : LinkState

    data class Failed(val reason: String) : LinkState
}

interface ComputerLink {
    val state: StateFlow<LinkState>

    fun start()

    fun stop()

    /** 짝 맺기 숫자를 확인한 결과 */
    fun answer(same: Boolean)
}
