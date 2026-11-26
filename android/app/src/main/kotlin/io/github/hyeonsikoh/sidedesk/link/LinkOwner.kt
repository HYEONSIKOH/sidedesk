package io.github.hyeonsikoh.sidedesk.link

import kotlinx.coroutines.flow.MutableStateFlow
import java.util.concurrent.atomic.AtomicReference

enum class Via { Usb, Bluetooth }

/**
 * 지금 화면을 쓰는 연결 하나를 정한다. USB 와 블루투스 받는 쪽이 같이 쓴다.
 * - USB 는 받는 순간 이어받는다(adb 를 거쳐 온 접속이라 믿을 수 있다).
 * - 블루투스는 키를 확인한(Ready) 뒤에만 이어받는다. USB 가 살아 있거나 다른 컴퓨터가 쓰고 있으면 진다.
 */
class LinkOwner {
    val state = MutableStateFlow<LinkState>(LinkState.Waiting)
    val answer = AtomicReference<Boolean?>(null)
    @Volatile var lastName: String? = null

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
        answer.set(null)
        return generation
    }

    @Synchronized
    fun ready(mine: Int, computer: ByteArray) {
        current?.takeIf { it.generation == mine }?.computer = computer
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

    @Synchronized
    fun closeAll() {
        current?.close?.invoke()
    }
}
