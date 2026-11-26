package io.github.hyeonsikoh.sidedesk.link

import android.content.Context
import android.net.LocalServerSocket
import android.net.LocalSocket
import android.system.ErrnoException
import android.system.Os
import android.system.OsConstants
import android.system.StructPollfd
import android.util.Log
import java.io.FileDescriptor
import java.io.IOException
import java.util.concurrent.Executors

/**
 * 컴퓨터가 접속해 오는 길 둘(USB, 블루투스)을 앱이 앞에 있을 때만 연다.
 * 어느 연결이 화면을 쓸지는 [LinkOwner] 가 정한다.
 */
class PhoneLink(context: Context, private val keys: KeyFiles) : ComputerLink {
    private val owner = LinkOwner()
    private val bluetooth = BleLink(context, owner, keys)
    override val state = owner.state
    override val bluetoothProblem = bluetooth.problem
    override val perf = owner.perf
    override val processes = owner.processes
    override val weather = owner.weather
    override val killResults = owner.killResults

    // 줄이 하나라서 이전 소켓을 닫은 뒤에만 같은 이름을 다시 연다.
    private val acceptor = Executors.newSingleThreadExecutor()
    private var listener: Listener? = null

    override fun start() {
        owner.pause(false)
        if (listener == null) listener = Listener().also { acceptor.execute(it) }
        bluetooth.start()
    }

    override fun stop() {
        owner.pause(true)
        bluetooth.stop()
        listener?.close()
        listener = null
    }

    override fun answer(same: Boolean) = owner.answer.set(same)

    override fun subscribe(subscription: Subscription) = owner.subscription.set(subscription)

    override fun setLocation(location: LocationState) = owner.setLocation(location)

    override fun kill(command: KillCommand) = owner.kill(command)

    override fun giveUp(commandId: Long) = owner.giveUp(commandId)

    /** 시작된 상태에서만 다시 연다(앱이 뒤에 있으면 앞으로 올 때 열린다). */
    override fun restartBluetooth() {
        if (listener == null) return
        bluetooth.stop()
        bluetooth.start()
    }

    /** adb 를 거쳐 오는 로컬 소켓을 받는다. */
    private inner class Listener : Runnable {
        private var closed = false
        private var server: LocalServerSocket? = null

        override fun run() {
            try {
                val opened = LocalServerSocket(NAME)
                try {
                    if (!register { server = opened }) return
                    if (state.value is LinkState.Failed) state.value = LinkState.Waiting
                    while (true) accept(opened.accept())
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

        private fun accept(socket: LocalSocket) {
            val uid = try {
                socket.peerCredentials.uid
            } catch (error: IOException) {
                -1
            }
            if (uid !in ADB_UIDS || synchronized(this) { closed }) {
                socket.close()
                return
            }
            val pipe = UsbPipe(socket)
            val mine = owner.takeUsb(pipe::close)
            Thread({ Connection(pipe, Via.Usb, owner, keys, mine).run() }, "sidedesk-usb").start()
        }

        private fun register(assign: () -> Unit): Boolean = synchronized(this) {
            if (!closed) assign()
            !closed
        }

        fun close() = synchronized(this) {
            closed = true
            // close 만으로는 기다리는 accept 와 read 가 깨지 않는다.
            shutdown(server?.fileDescriptor)
            owner.closeAll()
        }
    }

    private class UsbPipe(private val socket: LocalSocket) : Pipe {
        private val input = socket.inputStream
        private val output = socket.outputStream
        private val buffer = ByteArray(16 * 1024)

        override fun receive(): ByteArray? {
            if (!readable(socket.fileDescriptor)) return ByteArray(0)
            val count = input.read(buffer)
            return if (count < 0) null else buffer.copyOf(count)
        }

        override fun send(bytes: ByteArray) = output.write(bytes)

        override fun close() {
            shutdown(socket.fileDescriptor)
            runCatching { socket.close() }
        }
    }

    private companion object {
        const val TAG = "SideDeskLink"
        const val NAME = "sidedesk"
        const val POLL_MILLIS = 250
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
    }
}
