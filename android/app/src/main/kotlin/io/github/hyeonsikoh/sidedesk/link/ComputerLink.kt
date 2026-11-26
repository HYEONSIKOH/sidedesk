package io.github.hyeonsikoh.sidedesk.link

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
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

    /** 마지막으로 받은 값. 끊겨도 지우지 않는다. */
    val perf: StateFlow<Perf?>

    val processes: StateFlow<Processes?>

    val weather: StateFlow<Weather?>

    /** AI 세션 목록. 받은 적 없으면 null */
    val sessions: StateFlow<List<AiSession>?>

    val forge: StateFlow<Forge?>

    /** 세션 알림. 버리지 않는다. 한 연결 안의 같은 id 는 한 번만 온다. */
    val alerts: SharedFlow<AiAlert>

    /** 지금 붙은 컴퓨터의 단축키 목록. 다른 컴퓨터가 붙으면 지운다. */
    val shortcuts: StateFlow<List<Shortcut>?>

    /** 지금 컴퓨터의 목록에 있는 단축키면 명령으로 두고 true, 아니면 보내지 않고 false */
    fun runShortcut(command: RunShortcut): Boolean

    val results: SharedFlow<CommandResult>

    fun start()

    /** 연결이 바뀌어도 마지막 구독을 다시 보낸다. */
    fun subscribe(subscription: Subscription)

    /** 연결이 바뀌어도 마지막 위치를 다시 보낸다. Off 면 받아 둔 날씨도 지운다. */
    fun setLocation(location: LocationState)

    /** 결과가 올 때까지 다시 이어질 때마다 같은 번호로 보낸다. */
    fun send(command: Command)

    /** 기한 안에 결과가 없으면 부른다. 아직 남아 있으면 지우고 실패로 알린다. */
    fun giveUp(commandId: Long)

    fun stop()

    /** 등록 번호를 확인한 결과 */
    fun answer(same: Boolean)

    /** 블루투스 권한을 받은 뒤 다시 시작한다. */
    fun restartBluetooth() {}

    private companion object {
        val NO_PROBLEM: StateFlow<String?> = MutableStateFlow(null)
    }
}
