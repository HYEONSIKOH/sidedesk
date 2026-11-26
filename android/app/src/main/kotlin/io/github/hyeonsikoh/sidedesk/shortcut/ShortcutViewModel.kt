package io.github.hyeonsikoh.sidedesk.shortcut

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.github.hyeonsikoh.sidedesk.ai.Buzzer
import io.github.hyeonsikoh.sidedesk.desk.eul
import io.github.hyeonsikoh.sidedesk.link.CommandResult
import io.github.hyeonsikoh.sidedesk.link.ComputerLink
import io.github.hyeonsikoh.sidedesk.link.LinkOwner
import io.github.hyeonsikoh.sidedesk.link.LinkState
import io.github.hyeonsikoh.sidedesk.link.RunShortcut
import io.github.hyeonsikoh.sidedesk.link.Shortcut
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

data class ShortcutState(
    /** 지금 컴퓨터의 목록. 받은 적 없으면 null */
    val items: List<Shortcut>? = null,
    val dimmed: Boolean = false,
    /** 실패할 때만 한 줄. 4초 뒤 사라진다 */
    val notice: String? = null,
)

class ShortcutViewModel(
    private val link: ComputerLink,
    private val buzzer: Buzzer,
    /** 명령 번호. 앱을 다시 켜도 겹치지 않아야 한다. */
    private val newCommandId: () -> Long,
) : ViewModel() {
    private val _state = MutableStateFlow(ShortcutState())
    val state: StateFlow<ShortcutState> = _state.asStateFlow()

    private val names = mutableMapOf<Long, String>()
    private var clearing: Job? = null

    init {
        viewModelScope.launch { link.shortcuts.collect { list -> _state.update { it.copy(items = list) } } }
        viewModelScope.launch {
            link.state.collect { link -> _state.update { it.copy(dimmed = link !is LinkState.Connected) } }
        }
        viewModelScope.launch {
            link.results.collect { result ->
                val name = names.remove(result.commandId) ?: return@collect
                if (!result.ok) show(failure(name, result))
            }
        }
    }

    /** 진동 한 번 하고 번호만 보낸다. 성공은 말없이 넘어가고 실패만 알린다. */
    fun press(id: Int) {
        val item = _state.value.items?.firstOrNull { it.id == id } ?: return
        buzzer.tap()
        val command = RunShortcut(newCommandId(), id)
        if (!link.runShortcut(command)) {
            show(LinkOwner.NOT_CONNECTED)
            return
        }
        names[command.commandId] = item.name
        viewModelScope.launch {
            delay(LinkOwner.KILL_LIMIT_MILLIS)
            link.giveUp(command.commandId)
        }
    }

    private fun show(text: String) {
        _state.update { it.copy(notice = text) }
        clearing?.cancel()
        clearing = viewModelScope.launch {
            delay(NOTICE_MILLIS)
            _state.update { if (it.notice == text) it.copy(notice = null) else it }
        }
    }

    companion object {
        const val NOTICE_MILLIS = 4_000L
        private const val GENERIC = "실행하지 못했습니다"

        /** 이름이 비면 "단축키를", 이유가 비거나 "실행하지 못했습니다"면 겹치지 않게 뺀다. */
        fun failure(name: String, result: CommandResult): String {
            val head = "${if (name.isBlank()) "단축키를" else eul(name)} 실행하지 못했습니다"
            val reason = result.reason.trim()
            return if (reason.isEmpty() || reason == GENERIC) head else "$head. $reason"
        }
    }
}
