package io.github.hyeonsikoh.sidedesk.link

import android.util.Log
import uniffi.sidedesk_core.LinkException
import uniffi.sidedesk_core.PhoneEvent
import uniffi.sidedesk_core.PhoneSession
import java.io.IOException

/** 컴퓨터와 이어진 통로 하나. USB 와 블루투스가 이 모양으로 맞춘다. */
interface Pipe {
    /** 받은 바이트. 잠깐 기다려도 없으면 빈 배열, 끊겼으면 null */
    fun receive(): ByteArray?

    fun send(bytes: ByteArray)

    fun close()
}

/**
 * 통로 하나로 세션을 돌린다.
 * USB 는 받을 때 이미 이어받은 세대 번호를 넘긴다. 블루투스는 null 로 시작해 Ready 때 이어받는다.
 */
class Connection(
    private val pipe: Pipe,
    private val via: Via,
    private val owner: LinkOwner,
    private val keys: KeyFiles,
    private var mine: Int?,
    /** 블루투스는 이 시간 안에 Ready 가 되지 않으면 닫는다. */
    private val readyLimitNanos: Long? = null,
    /** 키를 확인하고 이어받은 뒤 부른다. */
    private val afterReady: () -> Unit = {},
) {
    fun run() {
        var ready = false
        Log.i(TAG, "$via 접속 시작")
        try {
            serve { ready = true }
            show(if (ready) LinkState.Lost(owner.lastName) else LinkState.Waiting)
        } catch (error: LinkException) {
            Log.w(TAG, "연결 오류", error)
            // 블루투스로 등록을 원하거나 모르는 컴퓨터면 화면을 바꾸지 않고 끊는다.
            show(LinkState.Failed(reason(error)))
        } catch (error: IOException) {
            Log.w(TAG, "접속이 끊김", error)
            show(if (ready) LinkState.Lost(owner.lastName) else LinkState.Waiting)
        } catch (error: Exception) {
            // 이 스레드에서 나간 예외는 앱을 끝낸다. 무엇이든 화면에 알리고 이 접속만 닫는다.
            Log.e(TAG, "예상 못 한 오류", error)
            show(LinkState.Failed("연결 오류"))
        } finally {
            pipe.close()
            mine?.let(owner::release)
            Log.i(TAG, "$via 접속 끝")
        }
    }

    private fun show(next: LinkState) {
        mine?.let { owner.show(it, next) }
    }

    private fun alive() = mine?.let(owner::isCurrent) ?: true

    private fun serve(onReady: () -> Unit) {
        val identity = keys.identity()
        val started = System.nanoTime()
        PhoneSession(identity.privateKey, identity.publicKey, keys.computers(), via == Via.Usb).use { session ->
            var heard = System.nanoTime()
            fun flush() = session.takeOutput().takeIf { it.isNotEmpty() }?.let(pipe::send)
            fun handleEvents(): Boolean {
                while (true) {
                    when (val event = session.nextEvent() ?: break) {
                        is PhoneEvent.PairingCode -> show(LinkState.Pairing(event.code))
                        is PhoneEvent.Paired -> keys.trust(event.computerKey)
                        is PhoneEvent.Ready -> {
                            if (mine == null) {
                                val took = owner.takeBluetooth(event.computerKey) { pipe.close() }
                                if (took == null) {
                                    Log.i(TAG, "블루투스 연결이 USB 나 다른 컴퓨터에 짐")
                                    return false
                                }
                                mine = took
                            } else {
                                owner.ready(mine!!, event.computerKey)
                            }
                            onReady()
                            afterReady()
                            show(LinkState.Connected(owner.lastName, via))
                        }
                        is PhoneEvent.ComputerName -> {
                            owner.lastName = event.name
                            show(LinkState.Connected(event.name, via))
                        }
                    }
                }
                return true
            }
            flush()
            while (alive()) {
                val limit = readyLimitNanos
                if (mine == null && limit != null && System.nanoTime() - started > limit) {
                    Log.i(TAG, "블루투스 확인 시간이 지남")
                    return
                }
                if (mine != null) {
                    owner.answer.getAndSet(null)?.let { same ->
                        try {
                            session.confirm(same)
                        } finally {
                            flush()
                        }
                        if (!handleEvents()) return
                    }
                }
                val bytes = pipe.receive() ?: return
                if (bytes.isNotEmpty()) {
                    heard = System.nanoTime()
                    session.feed(bytes)
                    if (mine == null) {
                        // 블루투스는 이어받을지 먼저 정한다. 질 연결이 Pong 을 보내 컴퓨터가 연결됨으로 알지 않게.
                        if (!handleEvents()) return
                        flush()
                    } else {
                        flush()
                        if (!handleEvents()) return
                    }
                } else if (System.nanoTime() - heard > SILENCE_NANOS) {
                    return
                }
            }
        }
    }

    companion object {
        const val TAG = "SideDeskLink"
        const val SILENCE_NANOS = 15_000_000_000L

        fun reason(error: LinkException) = when (error) {
            is LinkException.Version -> "컴퓨터와 버전이 다릅니다. 두 앱을 최신 버전으로 업데이트하세요"
            is LinkException.NotPaired -> "등록되지 않은 컴퓨터입니다. 컴퓨터의 SideDesk 설정에서 이 기기를 등록하세요"
            is LinkException.Rejected -> "등록을 취소했습니다"
            is LinkException.Broken -> "연결 오류: ${error.reason}"
        }
    }
}
