package io.github.hyeonsikoh.sidedesk.music

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.github.hyeonsikoh.sidedesk.ai.Buzzer
import io.github.hyeonsikoh.sidedesk.link.CommandResult
import io.github.hyeonsikoh.sidedesk.link.ComputerLink
import io.github.hyeonsikoh.sidedesk.link.LinkOwner
import io.github.hyeonsikoh.sidedesk.link.LinkState
import io.github.hyeonsikoh.sidedesk.link.MusicAction
import io.github.hyeonsikoh.sidedesk.link.MusicCommand
import io.github.hyeonsikoh.sidedesk.link.MusicPlayer
import io.github.hyeonsikoh.sidedesk.link.NowPlaying
import io.github.hyeonsikoh.sidedesk.link.Volume
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

data class MusicState(
    val now: NowPlaying? = null,
    val volume: Volume? = null,
    /** 지금 곡의 앨범 그림(jpeg). 없으면 기본 그림 */
    val art: ByteArray? = null,
    val dimmed: Boolean = false,
    /** 실패할 때만 한 줄. 4초 뒤 사라진다 */
    val notice: String? = null,
    /** 누르고 결과를 기다리는 동안 먼저 보이는 재생 여부 */
    val optimistic: Boolean? = null,
    /** 끄는 동안 손가락 볼륨 */
    val dragging: Int? = null,
    /** 끊긴 때(단조 밀리초). 진행 바를 여기서 멈춘다 */
    val lostAt: Long? = null,
) {
    val hasTrack: Boolean get() = now != null && now.player != MusicPlayer.None
    val playing: Boolean get() = optimistic ?: (now?.playing ?: false)
    val level: Int? get() = dragging ?: volume?.level

    override fun equals(other: Any?) = other is MusicState && now == other.now && volume == other.volume &&
        (art === other.art || (art != null && other.art != null && art.contentEquals(other.art))) &&
        dimmed == other.dimmed && notice == other.notice && optimistic == other.optimistic &&
        dragging == other.dragging && lostAt == other.lostAt

    override fun hashCode() = listOf(now, volume, art?.contentHashCode(), dimmed, notice, optimistic, dragging, lostAt).hashCode()
}

/** 진행 위치. 재생 중이면 받은 때부터 흐르고, 끊긴 때나 전체 길이에서 멈춘다. */
fun positionAt(now: NowPlaying, nowMs: Long, lostAt: Long? = null): Long {
    if (!now.playing) return now.positionMs
    val until = lostAt?.let { minOf(it, nowMs) } ?: nowMs
    val position = now.positionMs + (until - now.receivedAt).coerceAtLeast(0)
    return if (now.durationMs > 0) minOf(position, now.durationMs) else position
}

/** "1:24", 한 시간이 넘으면 "1:02:03" */
fun clockText(ms: Long): String {
    val total = ms.coerceAtLeast(0) / 1000
    val h = total / 3600
    val m = total % 3600 / 60
    val s = total % 60
    return if (h > 0) "%d:%02d:%02d".format(h, m, s) else "%d:%02d".format(m, s)
}

fun playerName(player: MusicPlayer) = if (player == MusicPlayer.Spotify) "Spotify" else "음악"

class MusicViewModel(
    private val link: ComputerLink,
    private val buzzer: Buzzer,
    private val newCommandId: () -> Long,
    /** 단조 밀리초. LinkOwner 가 받은 때를 적는 시계와 같아야 한다. */
    private val clock: () -> Long = { System.nanoTime() / 1_000_000 },
) : ViewModel() {
    private val _state = MutableStateFlow(MusicState())
    val state: StateFlow<MusicState> = _state.asStateFlow()

    private val pending = mutableMapOf<Long, MusicAction>()
    private var clearing: Job? = null
    private var settle: Job? = null
    private var lastVolumeSent = Long.MIN_VALUE / 2
    private var volumeLater: Job? = null
    private var volumeFailed = false

    init {
        viewModelScope.launch {
            combine(link.nowPlaying, link.artworks) { now, arts -> now to arts }.collect { (now, arts) ->
                settle?.cancel()
                _state.update {
                    it.copy(
                        now = now,
                        art = now?.artwork?.takeIf { hash -> hash != 0L }?.let(arts::get),
                        // 새 상태가 오면 먼저 보인 모양을 버리고 그것을 따른다.
                        optimistic = if (now != it.now) null else it.optimistic,
                    )
                }
            }
        }
        viewModelScope.launch {
            link.volume.collect { volume -> _state.update { it.copy(volume = volume) } }
        }
        viewModelScope.launch {
            link.state.collect { link ->
                val connected = link is LinkState.Connected
                _state.update { it.copy(dimmed = !connected, lostAt = if (connected) null else it.lostAt ?: clock()) }
            }
        }
        viewModelScope.launch { link.results.collect(::onResult) }
    }

    private fun canAct() = _state.value.let { it.hasTrack && !it.dimmed }

    /** 버튼 모양을 먼저 바꾸고 보낸다. 실패하거나 2초 안에 새 상태가 안 오면 되돌린다. */
    fun playPause() {
        val now = _state.value
        if (!canAct() || now.now?.canPlay != true) return
        val target = !now.playing
        if (!send(MusicAction.PlayPause)) return
        _state.update { it.copy(optimistic = target) }
    }

    fun next() {
        if (canAct() && _state.value.now?.canNext == true) send(MusicAction.Next)
    }

    fun previous() {
        if (canAct() && _state.value.now?.canPrevious == true) send(MusicAction.Previous)
    }

    /** 끄는 동안 0.1초에 한 번 마지막 값만 보낸다. 컴퓨터에서 온 값은 손을 뗄 때까지 무시한다. */
    fun dragVolume(level: Int) {
        val volume = _state.value.volume ?: return
        if (!volume.settable || _state.value.dimmed) return
        val value = level.coerceIn(0, 100)
        if (_state.value.dragging == null) volumeFailed = false
        _state.update { it.copy(dragging = value) }
        val wait = VOLUME_GAP_MILLIS - (clock() - lastVolumeSent)
        volumeLater?.cancel()
        if (wait <= 0) {
            sendVolume(value)
        } else {
            volumeLater = viewModelScope.launch {
                delay(wait)
                _state.value.dragging?.let(::sendVolume)
            }
        }
    }

    fun releaseVolume() {
        val value = _state.value.dragging ?: return
        volumeLater?.cancel()
        sendVolume(value)
        _state.update { it.copy(dragging = null) }
    }

    private fun sendVolume(level: Int) {
        lastVolumeSent = clock()
        val command = MusicCommand(newCommandId(), MusicAction.SetVolume, level)
        if (link.music(command)) pending[command.commandId] = MusicAction.SetVolume
    }

    private fun send(action: MusicAction): Boolean {
        buzzer.tap()
        val command = MusicCommand(newCommandId(), action)
        if (!link.music(command)) {
            show(LinkOwner.NOT_CONNECTED)
            return false
        }
        pending[command.commandId] = action
        viewModelScope.launch {
            delay(LinkOwner.KILL_LIMIT_MILLIS)
            link.giveUp(command.commandId)
        }
        return true
    }

    private fun onResult(result: CommandResult) {
        val action = pending.remove(result.commandId) ?: return
        when {
            action == MusicAction.SetVolume -> if (!result.ok && !volumeFailed) {
                volumeFailed = true
                show(failure(result))
            }
            !result.ok -> {
                if (action == MusicAction.PlayPause) _state.update { it.copy(optimistic = null) }
                show(failure(result))
            }
            action == MusicAction.PlayPause -> {
                settle?.cancel()
                settle = viewModelScope.launch {
                    delay(SETTLE_MILLIS)
                    _state.update { it.copy(optimistic = null) }
                }
            }
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
        const val VOLUME_GAP_MILLIS = 100L
        const val SETTLE_MILLIS = 2_000L

        fun failure(result: CommandResult) = result.reason.trim().ifEmpty { "실행하지 못했습니다" }
    }
}
