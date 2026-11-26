package io.github.hyeonsikoh.sidedesk.link

import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import java.util.concurrent.atomic.AtomicReference

enum class Via { Usb, Bluetooth }

/**
 * 지금 화면을 쓰는 연결 하나를 정한다. USB 와 블루투스 받는 쪽이 같이 쓴다.
 * - USB 는 받는 순간 이어받는다(adb 를 거쳐 온 접속이라 믿을 수 있다).
 * - 블루투스는 키를 확인한(Ready) 뒤에만 이어받는다. USB 가 살아 있거나 다른 컴퓨터가 쓰고 있으면 진다.
 */
class LinkOwner(private val nanos: () -> Long = System::nanoTime) {
    val state = MutableStateFlow<LinkState>(LinkState.Waiting)
    val answer = AtomicReference<Boolean?>(null)
    @Volatile var lastName: String? = null
    val perf = MutableStateFlow<Perf?>(null)
    val processes = MutableStateFlow<Processes?>(null)
    val weather = MutableStateFlow<Weather?>(null)
    val sessions = MutableStateFlow<List<AiSession>?>(null)
    val forge = MutableStateFlow<Forge?>(null)
    val alerts = MutableSharedFlow<AiAlert>(extraBufferCapacity = 64)
    /** 이 세대에서 받은 알림 번호. 새 연결이면 비운다(컴퓨터가 다시 켜지면 번호가 1부터 다시 간다). */
    private val seenAlerts = HashSet<Long>()
    private var seenFor = 0
    val location = AtomicReference<LocationState>(LocationState.Unknown)
    val killResults = MutableSharedFlow<KillResult>(extraBufferCapacity = 64)
    val subscription = AtomicReference(Subscription.NONE)
    private class Pending(val command: KillCommand, val computer: ByteArray, val at: Long)

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
        answer.set(null)
        return generation
    }

    @Synchronized
    fun ready(mine: Int, computer: ByteArray) {
        val now = current?.takeIf { it.generation == mine } ?: return
        now.computer = computer
        lastComputer = computer
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
    fun kill(command: KillCommand) {
        val queued = synchronized(this) {
            val computer = lastComputer ?: return@synchronized false
            kills[command.commandId] = Pending(command, computer, nanos())
            true
        }
        if (!queued) killResults.tryEmit(KillResult(command.commandId, false, NOT_CONNECTED))
    }

    /** 이 컴퓨터 앞으로 둔, 기한이 지나지 않은 명령 */
    @Synchronized
    fun pendingKills(computer: ByteArray): List<KillCommand> {
        val now = nanos()
        return kills.values
            .filter { it.computer.contentEquals(computer) && now - it.at <= KILL_LIMIT_NANOS }
            .map { it.command }
    }

    /** 기한 안에 결과가 오지 않은 명령을 지우고 실패로 알린다. */
    fun giveUp(commandId: Long) {
        val known = synchronized(this) { kills.remove(commandId) != null }
        if (known) killResults.tryEmit(KillResult(commandId, false, NOT_CONNECTED))
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
    fun receivedKill(result: KillResult) {
        val known = synchronized(this) { kills.remove(result.commandId) != null }
        if (known) killResults.tryEmit(result)
    }

    @Synchronized
    fun closeAll() {
        current?.close?.invoke()
    }

    companion object {
        const val KILL_LIMIT_MILLIS = 30_000L
        const val KILL_LIMIT_NANOS = KILL_LIMIT_MILLIS * 1_000_000
        const val NOT_CONNECTED = "컴퓨터와 연결되어 있지 않습니다"
    }
}
