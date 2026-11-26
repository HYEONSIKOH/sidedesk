package io.github.hyeonsikoh.sidedesk.rest

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.github.hyeonsikoh.sidedesk.ai.AiScreenState
import io.github.hyeonsikoh.sidedesk.clock.Settings
import io.github.hyeonsikoh.sidedesk.link.AiAlert
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** 쉬지 않게 막는 까닭. 하나라도 켜져 있으면 쉬지 않는다. */
enum class Hold { Picking, Confirm, Pairing, NoFocus }

data class RestState(
    val resting: Boolean = false,
    val keepOn: KeepOn = KeepOn.Charging,
    val restAfter: RestAfter = RestAfter.Five,
    val wakeOnAlert: Boolean = false,
    val vibration: Vibration = Vibration.Normal,
    /** 쉬는 화면에 어둡게 보이는 알림 */
    val alert: AiAlert? = null,
)

/**
 * 손을 안 대면 쉬고, 손대면 깬다. 진동은 하지 않는다(AI 화면 쪽 한 곳에서만 울린다).
 * 타이머는 delay 하나이고 손댈 때마다 다시 시작한다.
 */
class RestViewModel(private val settings: Settings, alerts: Flow<AiScreenState>) : ViewModel() {
    private val _state = MutableStateFlow(
        RestState(
            keepOn = KeepOn.entries.firstOrNull { it.name == settings.keepOn } ?: KeepOn.Charging,
            restAfter = RestAfter.entries.firstOrNull { it.name == settings.restAfter } ?: RestAfter.Five,
            wakeOnAlert = settings.wakeOnAlert,
            vibration = Vibration.entries.firstOrNull { it.name == settings.vibration } ?: Vibration.Normal,
        ),
    )
    val state: StateFlow<RestState> = _state.asStateFlow()

    private val holds = mutableSetOf<Hold>()
    private var visible = false
    private var timer: Job? = null
    private var shown: AiAlert? = null

    init {
        viewModelScope.launch {
            alerts.collect { ai ->
                val alert = ai.alert
                if (alert == null || alert == shown) {
                    if (alert == null) _state.update { it.copy(alert = null) }
                    return@collect
                }
                shown = alert
                when {
                    !_state.value.resting -> Unit
                    _state.value.wakeOnAlert && !ai.alertLate -> touched()
                    else -> _state.update { it.copy(alert = alert) }
                }
            }
        }
    }

    /** 누름(down)마다 부른다. 쉬는 중이면 깨우고, 타이머를 다시 잰다. */
    fun touched() {
        _state.update { it.copy(resting = false, alert = null) }
        restart()
    }

    /** 막는 까닭이 생기면 쉬는 중이어도 깨운다(짝 맺기 번호, 권한 창을 가리지 않게). */
    fun hold(reason: Hold, on: Boolean) {
        val changed = if (on) holds.add(reason) else holds.remove(reason)
        if (!changed) return
        if (on) _state.update { it.copy(resting = false, alert = null) }
        restart()
    }

    /** 화면이 안 보이면 타이머를 멈추고 깨어 있는 상태로 둔다. */
    fun setVisible(value: Boolean) {
        visible = value
        if (!value) _state.update { it.copy(resting = false, alert = null) }
        restart()
    }

    fun setKeepOn(value: KeepOn) {
        settings.keepOn = value.name
        _state.update { it.copy(keepOn = value) }
    }

    fun setRestAfter(value: RestAfter) {
        settings.restAfter = value.name
        _state.update { it.copy(restAfter = value) }
        restart()
    }

    fun setWakeOnAlert(value: Boolean) {
        settings.wakeOnAlert = value
        _state.update { it.copy(wakeOnAlert = value) }
    }

    fun setVibration(value: Vibration) {
        settings.vibration = value.name
        _state.update { it.copy(vibration = value) }
    }

    private fun restart() {
        timer?.cancel()
        val wait = _state.value.restAfter.millis ?: return
        if (!visible || holds.isNotEmpty() || _state.value.resting) return
        timer = viewModelScope.launch {
            delay(wait)
            _state.update { it.copy(resting = true) }
        }
    }
}
