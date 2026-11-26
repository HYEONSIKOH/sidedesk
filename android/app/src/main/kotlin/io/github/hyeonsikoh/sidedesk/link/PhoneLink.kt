package io.github.hyeonsikoh.sidedesk.link

import android.net.LocalServerSocket
import android.net.LocalSocket
import android.system.ErrnoException
import android.system.Os
import android.system.OsConstants
import android.system.StructPollfd
import android.util.Log
import kotlinx.coroutines.flow.MutableStateFlow
import uniffi.sidedesk_core.LinkException
import uniffi.sidedesk_core.PhoneEvent
import uniffi.sidedesk_core.PhoneSession
import java.io.FileDescriptor
import java.io.IOException
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference

/**
 * 컴퓨터가 adb 를 거쳐 접속해 오는 소켓. 앱이 앞에 있을 때만 연다.
 * 받는 스레드는 접속을 받기만 하고, 접속마다 다루는 스레드를 따로 둔다. 새 접속이 오면 이전 접속을 닫는다.
 */
class PhoneLink(private val keys: KeyFiles) : ComputerLink {
    override val state = MutableStateFlow<LinkState>(LinkState.Waiting)

    // 줄이 하나라서 이전 소켓을 닫은 뒤에만 같은 이름을 다시 연다.
    private val acceptor = Executors.newSingleThreadExecutor()
    private var listener: Listener? = null
    private val answer = AtomicReference<Boolean?>(null)
    private val generation = AtomicInteger()
    @Volatile private var lastName: String? = null

    override fun start() {
        if (listener != null) return
        listener = Listener().also { acceptor.execute(it) }
    }

    override fun stop() {
        listener?.close()
        listener = null
    }

    override fun answer(same: Boolean) = answer.set(same)

    private inner class Listener : Runnable {
        private var closed = false
        private var server: LocalServerSocket? = null
        private var client: LocalSocket? = null

        override fun run() {
            try {
                val opened = LocalServerSocket(NAME)
                try {
                    if (!register { server = opened }) return
                    if (state.value is LinkState.Failed) state.value = LinkState.Waiting
                    while (true) accept(opened.accept())
                } finally {
                    synchronized(this) { server = null }
                    shutdown(client?.fileDescriptor)
                    opened.close()
                }
            } catch (error: IOException) {
                if (synchronized(this) { closed }) return
                Log.w(TAG, "연결을 열지 못함", error)
                state.value = LinkState.Failed("연결을 열지 못함")
            }
        }

        private fun accept(socket: LocalSocket) {
            val uid = try {
                socket.peerCredentials.uid
            } catch (error: IOException) {
                -1
            }
            if (uid !in ADB_UIDS) {
                socket.close()
                return
            }
            val previous = synchronized(this) {
                if (closed) null.also { socket.close() } else client.also { client = socket }
            }
            if (synchronized(this) { closed }) return
            shutdown(previous?.fileDescriptor)
            val mine = generation.incrementAndGet()
            answer.set(null)
            Thread({ Connection(socket, mine).run() }, "sidedesk-link").start()
        }

        private fun register(assign: () -> Unit): Boolean = synchronized(this) {
            if (!closed) assign()
            !closed
        }

        fun close() = synchronized(this) {
            closed = true
            // close 만으로는 기다리는 accept 와 read 가 깨지 않는다.
            shutdown(server?.fileDescriptor)
            shutdown(client?.fileDescriptor)
        }
    }

    private inner class Connection(private val socket: LocalSocket, private val mine: Int) {
        private fun current() = generation.get() == mine

        private fun show(next: LinkState) {
            if (current()) state.value = next
        }

        fun run() {
            var ready = false
            try {
                socket.use { serve(it) { ready = true } }
                show(if (ready) LinkState.Lost(lastName) else LinkState.Waiting)
            } catch (error: LinkException) {
                Log.w(TAG, "연결 오류", error)
                show(LinkState.Failed(reason(error)))
            } catch (error: IOException) {
                Log.w(TAG, "접속이 끊김", error)
                show(if (ready) LinkState.Lost(lastName) else LinkState.Waiting)
            } catch (error: Exception) {
                // 이 스레드에서 나간 예외는 앱을 끝낸다. 무엇이든 화면에 알리고 이 접속만 닫는다.
                Log.e(TAG, "예상 못 한 오류", error)
                show(LinkState.Failed("연결 오류"))
            }
        }

        private fun serve(socket: LocalSocket, onReady: () -> Unit) {
            val identity = keys.identity()
            PhoneSession(identity.privateKey, identity.publicKey, keys.computers()).use { session ->
                val input = socket.inputStream
                val output = socket.outputStream
                val buffer = ByteArray(16 * 1024)
                var heard = System.nanoTime()
                fun flush() = session.takeOutput().takeIf { it.isNotEmpty() }?.let { output.write(it) }
                fun handleEvents() {
                    while (true) {
                        when (val event = session.nextEvent() ?: break) {
                            is PhoneEvent.PairingCode -> show(LinkState.Pairing(event.code))
                            is PhoneEvent.Paired -> keys.trust(event.computerKey)
                            is PhoneEvent.Ready -> {
                                onReady()
                                show(LinkState.Connected(lastName))
                            }
                            is PhoneEvent.ComputerName -> {
                                lastName = event.name
                                show(LinkState.Connected(event.name))
                            }
                        }
                    }
                }
                flush()
                while (current()) {
                    answer.getAndSet(null)?.let { same ->
                        try {
                            session.confirm(same)
                        } finally {
                            flush()
                        }
                        handleEvents()
                    }
                    if (readable(socket.fileDescriptor)) {
                        val count = input.read(buffer)
                        if (count < 0) return
                        heard = System.nanoTime()
                        session.feed(buffer.copyOf(count))
                        flush()
                        handleEvents()
                    } else if (System.nanoTime() - heard > SILENCE_NANOS) {
                        return
                    }
                }
            }
        }
    }

    private companion object {
        const val TAG = "SideDeskLink"
        const val NAME = "sidedesk"
        const val POLL_MILLIS = 250
        const val SILENCE_NANOS = 15_000_000_000L
        val ADB_UIDS = setOf(0, 2000)

        /** 읽을 것이 있거나 끊겼으면 true. 잠깐만 기다린다. */
        fun readable(descriptor: FileDescriptor): Boolean {
            val poll = StructPollfd().apply {
                fd = descriptor
                events = OsConstants.POLLIN.toShort()
            }
            return try {
                Os.poll(arrayOf(poll), POLL_MILLIS) > 0
            } catch (error: ErrnoException) {
                if (error.errno == OsConstants.EINTR) false else throw IOException(error)
            }
        }

        fun shutdown(descriptor: FileDescriptor?) {
            if (descriptor == null) return
            try {
                Os.shutdown(descriptor, OsConstants.SHUT_RDWR)
            } catch (error: ErrnoException) {
                Log.w(TAG, "이미 닫힘", error)
            }
        }

        fun reason(error: LinkException) = when (error) {
            is LinkException.Version -> "컴퓨터와 버전이 다름. 둘 다 최신으로 업데이트"
            is LinkException.NotPaired -> "짝을 맺지 않은 컴퓨터. 컴퓨터에서 sidedesk pair 를 실행"
            is LinkException.Rejected -> "짝 맺기를 취소함"
            is LinkException.Broken -> "연결 오류: ${error.reason}"
        }
    }
}
