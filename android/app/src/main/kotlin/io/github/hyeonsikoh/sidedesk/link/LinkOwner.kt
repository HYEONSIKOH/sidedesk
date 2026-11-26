package io.github.hyeonsikoh.sidedesk.link

import android.os.SystemClock
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import java.util.concurrent.atomic.AtomicReference

enum class Via { Usb, Bluetooth }

/**
 * 지금 화면을 쓰는 연결 하나를 정한다. USB 와 블루투스 받는 쪽이 같이 쓴다.
 * - USB 는 받는 순간 이어받는다(adb 를 거쳐 온 접속이라 믿을 수 있다).
 * - 블루투스는 키를 확인한(Ready) 뒤에만 이어받는다. USB 가 살아 있거나 다른 컴퓨터가 쓰고 있으면 진다.
 */
class LinkOwner(
    private val nanos: () -> Long = System::nanoTime,
    /** 음악 받은 때를 적는 시계(부팅 뒤 밀리초, 깊은 잠 포함). 화면이 꺼진 동안 흐른 재생 시간도 진행 바에 넣는다. */
    private val elapsed: () -> Long = SystemClock::elapsedRealtime,
) {
    val state = MutableStateFlow<LinkState>(LinkState.Waiting)
    val answer = AtomicReference<Boolean?>(null)
    @Volatile var lastName: String? = null
    val perf = MutableStateFlow<Perf?>(null)
    val processes = MutableStateFlow<Processes?>(null)
    val weather = MutableStateFlow<Weather?>(null)
    val sessions = MutableStateFlow<List<AiSession>?>(null)
    val forge = MutableStateFlow<Forge?>(null)
    val alerts = MutableSharedFlow<AiAlert>(extraBufferCapacity = 64)
    val shortcuts = MutableStateFlow<List<Shortcut>?>(null)
    /** 단축키 목록을 보낸 컴퓨터. 번호는 컴퓨터마다 따로라, 다른 컴퓨터에 보내지 않으려고 기억한다. */
    private var shortcutsFrom: ByteArray? = null
    /** 그 목록과 함께 온 장부 번호. 실행할 때 돌려주면 컴퓨터가 다른 장부의 번호를 거절한다. */
    private var shortcutsBook = 0L
    val nowPlaying = MutableStateFlow<NowPlaying?>(null)
    val volume = MutableStateFlow<Volume?>(null)
    /** 해시로 기억한 앨범 그림. 마지막 8장 */
    val artworks = MutableStateFlow<Map<Long, ByteArray>>(emptyMap())
    /** 곡과 볼륨을 보낸 컴퓨터. 다른 컴퓨터에 음악 명령을 보내지 않으려고 기억한다. */
    private var musicFrom: ByteArray? = null
    /** 바로 보낼 볼륨 하나. 다시 붙어도 보내지 않는다(늦은 값이 새 값을 덮지 않게). */
    private var volumeOut: MusicCommand? = null
    private var volumeFor = 0
    /** 결과를 기다리는 볼륨 명령 번호. 다시 보내지는 않지만 실패는 넘긴다. */
    private val volumeWaiting = HashSet<Long>()
    /** 이 세대에서 받은 알림 번호. 새 연결이면 비운다(컴퓨터가 다시 켜지면 번호가 1부터 다시 간다). */
    private val seenAlerts = HashSet<Long>()
    private var seenFor = 0
    val location = AtomicReference<LocationState>(LocationState.Unknown)
    val results = MutableSharedFlow<CommandResult>(extraBufferCapacity = 64)
    val subscription = AtomicReference(Subscription.NONE)
    private class Pending(val command: Command, val computer: ByteArray, val at: Long)

    private val kills = LinkedHashMap<Long, Pending>()
    /** 마지막으로 키를 확인한 컴퓨터. 끊긴 뒤에도 남겨 같은 컴퓨터가 다시 붙으면 밀린 명령을 보낸다. */
    private var lastComputer: ByteArray? = null

    private class Holder(val via: Via, var computer: ByteArray?, val close: () -> Unit, val generation: Int)

    private var generation = 0
    private var current: Holder? = null
    /** 앱이 뒤로 갔다. 확인 중이던 블루투스 접속이 이어받지 못하게 한다. */
    private var paused = false

    @Synchronized
    fun pause(value: Boolean) {
        paused = value
    }

    @Synchronized
    fun takeUsb(close: () -> Unit): Int {
        current?.close?.invoke()
        generation += 1
        current = Holder(Via.Usb, null, close, generation)
        answer.set(null)
        return generation
    }

    /** 이어받았으면 세대 번호, 졌으면 null */
    @Synchronized
    fun takeBluetooth(computer: ByteArray, close: () -> Unit): Int? {
        if (paused) return null
        val now = current
        if (now != null) {
            if (now.via == Via.Usb) return null
            val other = now.computer
            if (other != null && !other.contentEquals(computer)) return null
            now.close()
        }
        generation += 1
        current = Holder(Via.Bluetooth, computer, close, generation)
        lastComputer = computer
        forgetShortcutsOfOthers(computer)
        answer.set(null)
        return generation
    }

    @Synchronized
    fun ready(mine: Int, computer: ByteArray) {
        val now = current?.takeIf { it.generation == mine } ?: return
        now.computer = computer
        lastComputer = computer
        forgetShortcutsOfOthers(computer)
    }

    /** 다른 컴퓨터가 붙으면 앞 컴퓨터의 단축키 목록을 바로 지운다. 화면이 붙음으로 바뀌기 전이다. */
    private fun forgetShortcutsOfOthers(computer: ByteArray) {
        val music = musicFrom
        if (music != null && !music.contentEquals(computer)) {
            nowPlaying.value = null
            volume.value = null
            musicFrom = null
        }
        val from = shortcutsFrom ?: return
        if (from.contentEquals(computer)) return
        shortcuts.value = null
        shortcutsFrom = null
    }

    /** 받은 순간의 폰 시각을 붙여 둔다. */
    fun receivedNowPlaying(mine: Int, value: NowPlaying) = synchronized(this) {
        val now = current?.takeIf { it.generation == mine } ?: return@synchronized
        val computer = now.computer ?: return@synchronized
        nowPlaying.value = value.copy(receivedAt = elapsed())
        musicFrom = computer
    }

    fun receivedVolume(mine: Int, value: Volume) = synchronized(this) {
        val now = current?.takeIf { it.generation == mine } ?: return@synchronized
        val computer = now.computer ?: return@synchronized
        volume.value = value
        musicFrom = computer
    }

    fun receivedArtwork(mine: Int, hash: Long, jpeg: ByteArray) = synchronized(this) {
        if (generation != mine || hash == 0L) return@synchronized
        val kept = LinkedHashMap(artworks.value)
        kept.remove(hash)
        kept[hash] = jpeg
        while (kept.size > ARTWORKS) kept.remove(kept.keys.first())
        artworks.value = kept
    }

    /**
     * 보이는 곡(볼륨)이 지금 붙은 컴퓨터 것일 때만 명령을 둔다.
     * 재생/멈춤, 다음, 이전은 대기열(같은 번호 다시 보냄), 볼륨은 바로 보낼 칸에 마지막 값만 둔다.
     */
    fun music(command: MusicCommand): Boolean {
        val ok = synchronized(this) {
            val from = musicFrom
            val holder = current
            val now = holder?.computer
            val same = from != null && now != null && from.contentEquals(now)
            if (!same) return@synchronized false
            if (command.action == MusicAction.SetVolume) {
                volumeOut = command
                volumeFor = holder!!.generation
                volumeWaiting += command.commandId
                return@synchronized true
            }
            nowPlaying.value?.player?.let { it != MusicPlayer.None } ?: false
        }
        if (ok && command.action != MusicAction.SetVolume) send(command)
        return ok
    }

    /** 이 연결에 보낼 볼륨. 다른 연결에서 든 값은 버린다. */
    @Synchronized
    fun takeVolume(mine: Int): MusicCommand? {
        val out = volumeOut ?: return null
        volumeOut = null
        return out.takeIf { volumeFor == mine }
    }

    fun receivedShortcuts(mine: Int, book: Long, value: List<Shortcut>) = synchronized(this) {
        val now = current?.takeIf { it.generation == mine } ?: return@synchronized
        val computer = now.computer ?: return@synchronized
        shortcuts.value = value
        shortcutsFrom = computer
        shortcutsBook = book
    }

    /**
     * 지금 붙은 컴퓨터가 보낸 목록에 있는 단축키만 명령으로 둔다.
     * 목록이 다른 컴퓨터 것이거나, 붙어 있지 않거나, 목록에 없는 번호면 보내지 않고 false 다.
     */
    fun runShortcut(command: RunShortcut): Boolean {
        val booked = synchronized(this) {
            val from = shortcutsFrom
            val now = current?.computer
            val ok = from != null && now != null && from.contentEquals(now) &&
                shortcuts.value.orEmpty().any { it.id == command.shortcutId }
            if (ok) command.copy(book = shortcutsBook) else null
        } ?: return false
        send(booked)
        return true
    }

    @Synchronized
    fun release(mine: Int) {
        if (current?.generation == mine) current = null
    }

    @Synchronized
    fun isCurrent(mine: Int) = current?.generation == mine

    /** 화면 상태는 지금 연결만 바꾼다. */
    fun show(mine: Int, next: LinkState) {
        synchronized(this) {
            if (generation != mine) return
            state.value = next
        }
    }

    /** 지금 컴퓨터 앞으로 명령을 둔다. 아는 컴퓨터가 없으면 바로 실패로 알린다. */
    fun send(command: Command) {
        val queued = synchronized(this) {
            val computer = lastComputer ?: return@synchronized false
            kills[command.commandId] = Pending(command, computer, nanos())
            true
        }
        if (!queued) results.tryEmit(CommandResult(command.commandId, false, NOT_CONNECTED))
    }

    /** 이 컴퓨터 앞으로 둔, 기한이 지나지 않은 명령 */
    @Synchronized
    fun pendingCommands(computer: ByteArray): List<Command> {
        val now = nanos()
        return kills.values
            .filter { it.computer.contentEquals(computer) && now - it.at <= KILL_LIMIT_NANOS }
            .map { it.command }
    }

    /** 기한 안에 결과가 오지 않은 명령을 지우고 실패로 알린다. */
    fun giveUp(commandId: Long) {
        val known = synchronized(this) { kills.remove(commandId) != null }
        if (known) results.tryEmit(CommandResult(commandId, false, NOT_CONNECTED))
    }

    /** 받은 값은 지금 연결 것만 받는다. */
    fun receivedPerf(mine: Int, value: Perf) = synchronized(this) {
        if (generation == mine) perf.value = value
    }

    fun receivedProcesses(mine: Int, value: Processes) = synchronized(this) {
        if (generation == mine) processes.value = value
    }

    fun receivedSessions(mine: Int, value: List<AiSession>) = synchronized(this) {
        if (generation == mine) sessions.value = value
    }

    fun receivedForge(mine: Int, value: Forge) = synchronized(this) {
        if (generation == mine) forge.value = value
    }

    /** 지금 연결의 알림만, 같은 번호는 한 번만 넘긴다. */
    fun receivedAlert(mine: Int, value: AiAlert) {
        val fresh = synchronized(this) {
            if (generation != mine) return
            if (seenFor != mine) {
                seenAlerts.clear()
                seenFor = mine
            }
            seenAlerts.add(value.id)
        }
        if (fresh) alerts.tryEmit(value)
    }

    fun receivedWeather(mine: Int, value: Weather) = synchronized(this) {
        if (generation == mine && location.get() != LocationState.Off) weather.value = value
    }

    /** 권한을 거두면 다른 곳의 날씨를 흐리게 남기지 않도록 지운다. */
    fun setLocation(value: LocationState) = synchronized(this) {
        location.set(value)
        if (value == LocationState.Off) weather.value = null
    }

    /** 명령 결과는 어느 연결로 왔든 버리지 않는다. */
    fun receivedResult(result: CommandResult) {
        val known = synchronized(this) { kills.remove(result.commandId) != null || volumeWaiting.remove(result.commandId) }
        if (known) results.tryEmit(result)
    }

    @Synchronized
    fun closeAll() {
        current?.close?.invoke()
    }

    companion object {
        const val ARTWORKS = 8
        const val KILL_LIMIT_MILLIS = 30_000L
        const val KILL_LIMIT_NANOS = KILL_LIMIT_MILLIS * 1_000_000
        const val NOT_CONNECTED = "컴퓨터와 연결되어 있지 않습니다"
    }
}
