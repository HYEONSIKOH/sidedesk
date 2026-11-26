package io.github.hyeonsikoh.sidedesk.link

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow

sealed interface LinkState {
    data object Waiting : LinkState

    /** 컴퓨터에 뜬 번호와 같은지 사람이 확인해야 한다. */
    data class Pairing(val code: String) : LinkState

    data class Connected(val computerName: String?, val via: Via = Via.Usb) : LinkState

    /** 끊겼다. 마지막 이름을 흐리게 보인다. */
    data class Lost(val computerName: String?) : LinkState

    data class Failed(val reason: String) : LinkState
}

interface ComputerLink {
    val state: StateFlow<LinkState>

    /** 블루투스를 쓸 수 없는 이유. USB 는 그대로 쓴다. */
    val bluetoothProblem: StateFlow<String?> get() = NO_PROBLEM

    fun start()

    fun stop()

    /** 등록 번호를 확인한 결과 */
    fun answer(same: Boolean)

    /** 블루투스 권한을 받은 뒤 다시 시작한다. */
    fun restartBluetooth() {}

    private companion object {
        val NO_PROBLEM: StateFlow<String?> = MutableStateFlow(null)
    }
}
