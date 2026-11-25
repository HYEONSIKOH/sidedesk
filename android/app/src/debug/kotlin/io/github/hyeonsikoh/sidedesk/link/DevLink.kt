package io.github.hyeonsikoh.sidedesk.link

import android.net.LocalServerSocket
import android.net.LocalSocket
import android.system.ErrnoException
import android.system.Os
import android.system.OsConstants
import android.util.Log
import kotlinx.coroutines.flow.MutableStateFlow
import uniffi.sidedesk_core.PhoneReceiver
import uniffi.sidedesk_core.ReceiveException
import java.io.FileDescriptor
import java.io.IOException
import java.util.concurrent.Executors

/** 암호화가 없는 개발용 연결. SD-2 에서 지운다. */
class DevLink : ComputerLink {
    override val state = MutableStateFlow<LinkState>(LinkState.Waiting)

    // 스레드가 하나라서 이전 소켓을 닫은 뒤에만 같은 이름을 다시 연다.
    private val worker = Executors.newSingleThreadExecutor()
    private var session: Session? = null

    override fun start() {
        if (session != null) return
        session = Session().also { worker.execute(it) }
    }

    override fun stop() {
        session?.close()
        session = null
    }

    private inner class Session : Runnable {
        private var closed = false
        private var server: LocalServerSocket? = null
        private var client: LocalSocket? = null

        override fun run() {
            try {
                val opened = LocalServerSocket(NAME)
                try {
                    if (!register { server = opened }) return
                    if (state.value is LinkState.Failed) state.value = LinkState.Waiting
                    while (true) serve(opened.accept())
                } finally {
                    synchronized(this) { server = null }
                    opened.close()
                }
            } catch (error: IOException) {
                if (synchronized(this) { closed }) return
                Log.w(TAG, "연결을 열지 못함", error)
                state.value = LinkState.Failed("연결을 열지 못함")
            }
        }

        private fun serve(socket: LocalSocket) = socket.use {
            if (!register { client = socket }) return@use
            try {
                if (socket.peerCredentials.uid in ADB_UIDS) read(socket)
            } catch (error: IOException) {
                Log.w(TAG, "접속이 끊김", error)
            } catch (error: ReceiveException) {
                Log.w(TAG, "읽을 수 없는 메시지", error)
            } finally {
                synchronized(this) { client = null }
            }
        }

        private fun read(socket: LocalSocket) {
            socket.soTimeout = READ_TIMEOUT
            val buffer = ByteArray(4096)
            PhoneReceiver().use { receiver ->
                while (true) {
                    val count = socket.inputStream.read(buffer)
                    if (count < 0) return
                    receiver.receive(buffer.copyOf(count)).lastOrNull()?.let {
                        state.value = LinkState.Received(it)
                    }
                }
            }
        }

        private fun register(assign: () -> Unit): Boolean = synchronized(this) {
            if (!closed) assign()
            !closed
        }

        fun close() = synchronized(this) {
            closed = true
            // close 만으로는 기다리는 accept 와 read 가 깨지 않는다.
            server?.let { shutdown(it.fileDescriptor) }
            client?.let { shutdown(it.fileDescriptor) }
        }

        private fun shutdown(descriptor: FileDescriptor) {
            try {
                Os.shutdown(descriptor, OsConstants.SHUT_RDWR)
            } catch (error: ErrnoException) {
                Log.w(TAG, "이미 닫힘", error)
            }
        }
    }

    private companion object {
        const val TAG = "SideDeskLink"
        const val NAME = "sidedesk-dev"
        const val READ_TIMEOUT = 5_000
        val ADB_UIDS = setOf(0, 2000)
    }
}
