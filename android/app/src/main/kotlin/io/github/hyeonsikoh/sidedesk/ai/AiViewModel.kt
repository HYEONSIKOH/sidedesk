package io.github.hyeonsikoh.sidedesk.ai

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.github.hyeonsikoh.sidedesk.link.AiAlert
import io.github.hyeonsikoh.sidedesk.link.AiSession
import io.github.hyeonsikoh.sidedesk.link.AiState
import io.github.hyeonsikoh.sidedesk.link.ComputerLink
import io.github.hyeonsikoh.sidedesk.link.Forge
import io.github.hyeonsikoh.sidedesk.link.ForgeCounts
import io.github.hyeonsikoh.sidedesk.link.LinkState
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

enum class ForgeTab { GitHub, GitLab }

data class AiScreenState(
    /** 작업 중과 답변 대기 먼저, 그다음 끝남과 알 수 없음. 같은 무리 안에서는 최근 것 먼저 */
    val sessions: List<AiSession> = emptyList(),
    /** 작은 칸 숫자(작업 중 + 답변 대기). 받은 적 없으면 null */
    val busy: Int? = null,
    val waiting: Boolean = false,
    val forge: Forge? = null,
    val tab: ForgeTab = ForgeTab.GitHub,
    /** 지금 띄운 알림. 4초 뒤 사라진다 */
    val alert: AiAlert? = null,
    val dimmed: Boolean = false,
) {
    val counts: ForgeCounts? get() = forge?.let { if (tab == ForgeTab.GitHub) it.github else it.gitlab }
}

/** 작업 중과 답변 대기는 큰 카드, 나머지는 작은 카드 */
fun AiSession.active() = state == AiState.Working || state == AiState.Waiting

class AiViewModel(
    private val link: ComputerLink,
    private val buzzer: Buzzer,
    private val now: () -> Long = System::currentTimeMillis,
) : ViewModel() {
    private val _state = MutableStateFlow(AiScreenState())
    val state: StateFlow<AiScreenState> = _state.asStateFlow()

    private var lastBuzz = Long.MIN_VALUE / 2
    private var clearing: Job? = null

    init {
        viewModelScope.launch {
            link.sessions.collect { list ->
                if (list == null) return@collect
                _state.update {
                    it.copy(
                        sessions = sorted(list),
                        busy = list.count(AiSession::active),
                        waiting = list.any { session -> session.state == AiState.Waiting },
                    )
                }
            }
        }
        viewModelScope.launch { link.forge.collect { forge -> if (forge != null) _state.update { it.copy(forge = forge) } } }
        viewModelScope.launch {
            link.state.collect { link -> _state.update { it.copy(dimmed = link !is LinkState.Connected) } }
        }
        // 화면과 상관없이 처음부터 모은다. 알림 흐름은 다시 보내 주지 않으므로 늦게 모으면 사라진다.
        viewModelScope.launch { link.alerts.collect(::show) }
    }

    fun selectTab(tab: ForgeTab) = _state.update { it.copy(tab = tab) }

    /** 화면은 넘기지 않는다. 4초 동안 보이고, 늦게 온 알림은 깨우지 않는다. */
    private fun show(alert: AiAlert) {
        val time = now()
        val late = time - alert.at > LATE_MILLIS
        if (!late && time - lastBuzz >= QUIET_MILLIS) {
            buzzer.buzz(alert.kind)
            lastBuzz = time
        }
        _state.update { it.copy(alert = alert) }
        clearing?.cancel()
        clearing = viewModelScope.launch {
            delay(SHOW_MILLIS)
            _state.update { if (it.alert == alert) it.copy(alert = null) else it }
        }
    }

    private fun sorted(list: List<AiSession>) =
        list.sortedWith(compareBy<AiSession> { !it.active() }.thenByDescending { it.since })

    companion object {
        const val SHOW_MILLIS = 4_000L
        const val LATE_MILLIS = 30_000L
        const val QUIET_MILLIS = 2_000L
    }
}

/** 컴퓨터는 알림을 50 개까지만 센다. 50 이면 그보다 많을 수 있다. */
const val NOTICE_CAP = 50

fun noticeText(count: Int) = if (count >= NOTICE_CAP) "$NOTICE_CAP+" else "$count"

/** 지난 시간 글자. 1분 안은 "지금", 그다음 "12분", "7시간 4분", "1일 16시간" */
fun elapsedText(now: Long, since: Long): String {
    val minutes = (now - since).coerceAtLeast(0) / 60_000
    val hours = minutes / 60
    val days = hours / 24
    return when {
        minutes < 1 -> "지금"
        hours < 1 -> "${minutes}분"
        days < 1 -> if (minutes % 60 == 0L) "${hours}시간" else "${hours}시간 ${minutes % 60}분"
        else -> if (hours % 24 == 0L) "${days}일" else "${days}일 ${hours % 24}시간"
    }
}
