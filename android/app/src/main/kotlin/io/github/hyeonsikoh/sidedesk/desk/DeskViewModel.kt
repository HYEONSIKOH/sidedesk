package io.github.hyeonsikoh.sidedesk.desk

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.github.hyeonsikoh.sidedesk.link.ComputerLink
import io.github.hyeonsikoh.sidedesk.link.KillCommand
import io.github.hyeonsikoh.sidedesk.link.CommandResult
import io.github.hyeonsikoh.sidedesk.link.LinkState
import io.github.hyeonsikoh.sidedesk.link.Perf
import io.github.hyeonsikoh.sidedesk.link.ProcessInfo
import io.github.hyeonsikoh.sidedesk.link.Subscription
import io.github.hyeonsikoh.sidedesk.link.Topic
import io.github.hyeonsikoh.sidedesk.link.LinkOwner
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

enum class Sort { Cpu, Memory }

/** 큰 화면에 띄울 수 있는 것. 옆으로 밀면 이 차례로 돈다. */
enum class Screen(val topic: Topic) {
    Home(Topic.Weather),
    Ai(Topic.Ai),
    Perf(Topic.Perf),
    Processes(Topic.Processes),
    Shortcuts(Topic.Shortcuts),
    Music(Topic.Music),
}

data class DeskState(
    val big: Screen = Screen.Home,
    val perf: Perf? = null,
    /** 오래된 것부터. 그래프용 */
    val received: List<Long> = emptyList(),
    val sent: List<Long> = emptyList(),
    val sort: Sort = Sort.Cpu,
    val rows: List<ProcessInfo> = emptyList(),
    /** 끝낼지 묻는 중인 프로세스 */
    val asking: ProcessInfo? = null,
    val notice: String? = null,
    /** 끊겨서 마지막 값을 흐리게 보인다 */
    val dimmed: Boolean = false,
) {
    /** 오른쪽 작은 칸. 시계는 늘 맨 위, 큰 화면에 띄운 것은 뺀다. */
    val small: List<Screen> get() = Screen.entries.filter { it != big }
}

/** 이름 뒤 조사. 한글로 끝나면 받침에 맞추고, 아니면 둘 다 적는다. */
internal fun eul(name: String) = name + particle(name, "을", "를", "을(를)")

private fun particle(name: String, withFinal: String, without: String, unknown: String): String {
    val last = name.lastOrNull() ?: return unknown
    if (last !in '가'..'힣') return unknown
    return if ((last - '가') % 28 != 0) withFinal else without
}

class DeskViewModel(
    private val link: ComputerLink,
    /** 명령 번호. 앱을 다시 켜도 겹치지 않아야 한다. */
    private val newCommandId: () -> Long,
) : ViewModel() {
    private val _state = MutableStateFlow(DeskState())
    val state: StateFlow<DeskState> = _state.asStateFlow()

    private var visible = false
    private var allProcesses: List<ProcessInfo> = emptyList()
    private val names = mutableMapOf<Long, String>()

    init {
        viewModelScope.launch {
            link.perf.collect { perf ->
                if (perf == null) return@collect
                _state.update {
                    it.copy(
                        perf = perf,
                        received = (it.received + perf.receivePerSecond).takeLast(HISTORY),
                        sent = (it.sent + perf.sendPerSecond).takeLast(HISTORY),
                    )
                }
            }
        }
        viewModelScope.launch {
            link.processes.collect { processes ->
                if (processes == null) return@collect
                allProcesses = processes.list
                _state.update {
                    it.copy(rows = sorted(processes.list, it.sort))
                }
            }
        }
        viewModelScope.launch {
            link.state.collect { link -> _state.update { it.copy(dimmed = link !is LinkState.Connected) } }
        }
        viewModelScope.launch {
            link.results.collect { result ->
                val name = names.remove(result.commandId) ?: return@collect
                _state.update { it.copy(notice = message(name, result)) }
            }
        }
    }

    /** 화면이 보이는지. 안 보이면 구독을 비운다. */
    fun setVisible(value: Boolean) {
        visible = value
        tellComputer()
    }

    /** 프로세스 끝내기를 묻는 중에는 화면을 바꾸지 않는다. */
    fun show(screen: Screen) {
        if (_state.value.asking != null) return
        _state.update { it.copy(big = screen) }
        tellComputer()
    }

    fun next() = show(Screen.entries[(_state.value.big.ordinal + 1) % Screen.entries.size])

    fun previous() = show(Screen.entries[(_state.value.big.ordinal + Screen.entries.size - 1) % Screen.entries.size])

    fun sortBy(sort: Sort) = _state.update { it.copy(sort = sort, rows = sorted(allProcesses, sort)) }

    fun ask(process: ProcessInfo) = _state.update { it.copy(asking = process) }

    fun cancel() = _state.update { it.copy(asking = null) }

    fun confirmKill() {
        val target = _state.value.asking ?: return
        val id = newCommandId()
        names[id] = target.name
        link.send(KillCommand(id, target.pid, target.startedAt))
        viewModelScope.launch {
            delay(LinkOwner.KILL_LIMIT_MILLIS)
            link.giveUp(id)
        }
        _state.update { it.copy(asking = null, notice = "${eul(target.name)} 끝내는 중") }
    }

    fun dismissNotice() = _state.update { it.copy(notice = null) }

    private var resting = false

    /** 쉬는 동안은 시계와 AI 알림만 쓰므로 날씨와 AI 만 받는다. 깨면 원래 표로 돌아간다. */
    fun setResting(value: Boolean) {
        if (resting == value) return
        resting = value
        tellComputer()
    }

    private fun tellComputer() {
        val now = _state.value
        link.subscribe(
            when {
                !visible -> Subscription.NONE
                resting -> Subscription(Topic.Weather, setOf(Topic.Ai))
                else -> Subscription(now.big.topic, now.small.map { it.topic }.toSet())
            },
        )
    }

    private fun sorted(list: List<ProcessInfo>, sort: Sort) = when (sort) {
        Sort.Cpu -> list.sortedByDescending { it.cpu }
        Sort.Memory -> list.sortedByDescending { it.memoryBytes }
    }

    private fun message(name: String, result: CommandResult) = when {
        result.ok -> "${eul(name)} 끝냈습니다"
        result.reason.isBlank() -> "${eul(name)} 끝내지 못했습니다"
        else -> "${eul(name)} 끝내지 못했습니다. ${result.reason}"
    }

    companion object {
        const val HISTORY = 60
    }
}
