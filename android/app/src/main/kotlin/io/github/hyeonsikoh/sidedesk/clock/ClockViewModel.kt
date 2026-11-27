package io.github.hyeonsikoh.sidedesk.clock

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.github.hyeonsikoh.sidedesk.link.ComputerLink
import io.github.hyeonsikoh.sidedesk.link.LinkState
import io.github.hyeonsikoh.sidedesk.link.Via
import io.github.hyeonsikoh.sidedesk.link.Weather
import io.github.hyeonsikoh.sidedesk.link.WeatherSource
import io.github.hyeonsikoh.sidedesk.ui.Theme
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.transformLatest
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

enum class Face(val label: String) {
    Digits("숫자"),
    Dial("바늘"),
    Weather("날씨"),
    Calendar("달력"),
    Overview("모아 보기"),
}

data class ClockState(
    val face: Face = Face.Digits,
    val theme: Theme = Theme.Dark,
    val picking: Boolean = false,
    /** 날짜 옆 아이콘. 끊기면 null */
    val via: Via? = null,
    val weather: Weather? = null,
    /** 끊겼거나 받은 지 오래된 날씨 */
    val weatherDimmed: Boolean = false,
) {
    /** 날씨 옆 출처. 날씨가 없거나 받은 곳을 모르면 null. 기상청은 표시하지 않는다. */
    val credit: String? get() = when (weather?.source) {
        WeatherSource.OpenMeteo -> "날씨 자료 Open-Meteo.com"
        else -> null
    }

    /** 작은 칸에 쓰는 짧은 출처. 기상청은 표시하지 않는다. */
    val shortCredit: String? get() = when (weather?.source) {
        WeatherSource.OpenMeteo -> "Open-Meteo"
        else -> null
    }
}

@OptIn(ExperimentalCoroutinesApi::class)
class ClockViewModel(
    private val link: ComputerLink,
    private val settings: Settings,
    private val now: () -> Long = System::currentTimeMillis,
) : ViewModel() {
    private val _state = MutableStateFlow(
        ClockState(
            face = Face.entries.firstOrNull { it.name == settings.face } ?: Face.Digits,
            theme = Theme.entries.firstOrNull { it.name == settings.theme } ?: Theme.Dark,
        ),
    )
    val state: StateFlow<ClockState> = _state.asStateFlow()
    private val visible = MutableStateFlow(true)

    init {
        // 받기가 계속 실패해 오래된 값도 흐리게 하려고, 보이는 동안만 1분마다 다시 본다.
        val minutes = visible.transformLatest { shown ->
            emit(Unit)
            while (shown) {
                delay(60_000)
                emit(Unit)
            }
        }
        viewModelScope.launch {
            combine(link.state, link.weather, minutes) { state, weather, _ -> state to weather }.collect { (state, weather) ->
                val connected = state as? LinkState.Connected
                _state.update {
                    it.copy(
                        via = connected?.via,
                        weather = weather,
                        weatherDimmed = connected == null || (weather != null && stale(weather)),
                    )
                }
            }
        }
    }

    /** 화면이 보이는지. 안 보이면 1분 확인을 멈춘다. */
    fun setVisible(value: Boolean) {
        visible.value = value
    }

    fun pick() = _state.update { it.copy(picking = true) }

    fun done() = _state.update { it.copy(picking = false) }

    fun setFace(face: Face) {
        settings.face = face.name
        _state.update { it.copy(face = face) }
    }

    /** 고르는 중 옆으로 밀면 모양이 돈다. */
    fun nextFace() = turn(1)

    fun previousFace() = turn(Face.entries.size - 1)

    fun setTheme(theme: Theme) {
        settings.theme = theme.name
        _state.update { it.copy(theme = theme) }
    }

    private fun turn(step: Int) {
        if (!_state.value.picking) return
        setFace(Face.entries[(_state.value.face.ordinal + step) % Face.entries.size])
    }

    /** 폰과 맥의 시계 차이를 생각해 1시간 넘게 지난 값만 오래된 것으로 본다. 앞선 시각은 새것이다. */
    private fun stale(weather: Weather) = now() - weather.fetchedAt > STALE_MILLIS

    companion object {
        const val STALE_MILLIS = 60 * 60 * 1_000L
    }
}
