package io.github.hyeonsikoh.sidedesk.link

import android.Manifest
import android.annotation.SuppressLint
import android.bluetooth.BluetoothAdapter
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothGattServer
import android.bluetooth.BluetoothGattServerCallback
import android.bluetooth.BluetoothGattService
import android.bluetooth.BluetoothManager
import android.bluetooth.BluetoothServerSocket
import android.bluetooth.BluetoothSocket
import android.bluetooth.le.AdvertiseCallback
import android.bluetooth.le.AdvertiseData
import android.bluetooth.le.AdvertiseSettings
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageManager
import android.os.Build
import android.os.ParcelUuid
import android.util.Log
import kotlinx.coroutines.flow.MutableStateFlow
import uniffi.sidedesk_core.bleServiceData
import uniffi.sidedesk_core.bleServiceUuid
import java.util.UUID
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

/**
 * 블루투스 LE 로 자기를 알리고(알림 신호), 컴퓨터가 L2CAP 채널로 붙어 오면 받는다.
 * 블루투스로는 등록하지 않고, 키를 확인하기 전에는 지금 연결과 화면을 건드리지 않는다.
 */
@SuppressLint("MissingPermission")
class BleLink(
    private val context: Context,
    private val owner: LinkOwner,
    private val keys: KeyFiles,
) {
    val problem = MutableStateFlow<String?>(null)

    private val service = UUID.fromString(bleServiceUuid())
    private var running = false
    private var server: BluetoothServerSocket? = null
    private var gatt: BluetoothGattServer? = null
    private var advertising: AdvertiseCallback? = null
    private var watching: BroadcastReceiver? = null

    /** Ready 전 접속은 하나만 받는다. */
    private val checking = AtomicBoolean(false)

    @Synchronized
    fun start() {
        if (running) return
        running = true
        watchPower()
        open()
    }

    @Synchronized
    fun stop() {
        running = false
        watching?.let { runCatching { context.unregisterReceiver(it) } }
        watching = null
        close()
    }

    private fun adapter(): BluetoothAdapter? = context.getSystemService(BluetoothManager::class.java)?.adapter

    private fun allowed(): Boolean {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S) return true
        return listOf(Manifest.permission.BLUETOOTH_ADVERTISE, Manifest.permission.BLUETOOTH_CONNECT).all {
            context.checkSelfPermission(it) == PackageManager.PERMISSION_GRANTED
        }
    }

    @Synchronized
    private fun open() {
        if (!running || server != null) return
        val adapter = adapter() ?: return say("이 기기는 블루투스가 없어 USB 로만 연결합니다")
        if (!allowed()) return say("블루투스 권한이 없어 USB 로만 연결합니다")
        if (!adapter.isEnabled) return say("블루투스가 꺼져 있어 USB 로만 연결합니다")
        val advertiser = adapter.bluetoothLeAdvertiser
            ?: return say("이 기기는 블루투스 알림 신호를 낼 수 없어 USB 로만 연결합니다")
        val opened = try {
            adapter.listenUsingInsecureL2capChannel()
        } catch (error: Exception) {
            Log.w(TAG, "L2CAP 를 열지 못함", error)
            return say("블루투스 연결을 열지 못해 USB 로만 연결합니다")
        }
        server = opened
        // 맥이 붙을 때 GATT 를 살피므로 빈 서비스 하나를 둔다.
        gatt = context.getSystemService(BluetoothManager::class.java)
            .openGattServer(context, object : BluetoothGattServerCallback() {
                override fun onConnectionStateChange(device: BluetoothDevice, status: Int, newState: Int) {
                    Log.i(TAG, "블루투스 연결 상태 $newState")
                }
            })
            ?.also { it.addService(BluetoothGattService(service, BluetoothGattService.SERVICE_TYPE_PRIMARY)) }
        val data = AdvertiseData.Builder()
            .setIncludeDeviceName(false)
            .addServiceData(ParcelUuid(service), bleServiceData(opened.psm.toUShort(), keys.identity().publicKey))
            .build()
        advertise(advertiser, data, withName = true)
        Thread({ accept(opened) }, "sidedesk-ble-accept").start()
    }

    /** 기기 이름이 길어 스캔 응답에 안 들어가면 이름 없이 다시 알린다. */
    private fun advertise(advertiser: android.bluetooth.le.BluetoothLeAdvertiser, data: AdvertiseData, withName: Boolean) {
        val response = AdvertiseData.Builder().setIncludeDeviceName(withName).build()
        val settings = AdvertiseSettings.Builder()
            .setAdvertiseMode(AdvertiseSettings.ADVERTISE_MODE_BALANCED)
            .setConnectable(true)
            .build()
        val callback = object : AdvertiseCallback() {
            override fun onStartSuccess(settingsInEffect: AdvertiseSettings) {
                problem.value = null
            }

            override fun onStartFailure(errorCode: Int) {
                Log.w(TAG, "알림 신호 실패 $errorCode")
                if (withName && errorCode == ADVERTISE_FAILED_DATA_TOO_LARGE) {
                    synchronized(this@BleLink) { if (running && server != null) advertise(advertiser, data, false) }
                    return
                }
                problem.value = "블루투스 알림 신호를 내지 못해 USB 로만 연결합니다"
            }
        }
        advertising = callback
        advertiser.startAdvertising(settings, data, response, callback)
    }

    @Synchronized
    private fun close() {
        advertising?.let { callback -> runCatching { adapter()?.bluetoothLeAdvertiser?.stopAdvertising(callback) } }
        advertising = null
        runCatching { server?.close() }
        server = null
        runCatching { gatt?.close() }
        gatt = null
    }

    private fun say(text: String) {
        Log.i(TAG, text)
        problem.value = text
    }

    private fun accept(opened: BluetoothServerSocket) {
        while (true) {
            val socket = try {
                opened.accept()
            } catch (error: Exception) {
                return
            }
            if (!checking.compareAndSet(false, true)) {
                Log.i(TAG, "확인 중인 블루투스 접속이 있어 새 접속을 닫음")
                runCatching { socket.close() }
                continue
            }
            Thread({
                // 이 접속이 잡은 확인 자리는 이 접속이 한 번만 놓는다.
                val held = AtomicBoolean(true)
                val letGo = { if (held.compareAndSet(true, false)) checking.set(false) }
                val pipe = BlePipe(socket)
                Connection(pipe, Via.Bluetooth, owner, keys, null, READY_LIMIT_NANOS, letGo).run()
                letGo()
            }, "sidedesk-ble").start()
        }
    }

    /** 블루투스가 꺼졌다 켜지면 다시 연다. */
    private fun watchPower() {
        val receiver = object : BroadcastReceiver() {
            override fun onReceive(context: Context, intent: Intent) {
                when (intent.getIntExtra(BluetoothAdapter.EXTRA_STATE, BluetoothAdapter.ERROR)) {
                    BluetoothAdapter.STATE_ON -> open()
                    BluetoothAdapter.STATE_OFF -> {
                        close()
                        say("블루투스가 꺼져 있어 USB 로만 연결합니다")
                    }
                }
            }
        }
        watching = receiver
        context.registerReceiver(receiver, IntentFilter(BluetoothAdapter.ACTION_STATE_CHANGED))
    }

    /** L2CAP 소켓. 읽기는 막고 기다리는 스레드 하나가 받아 큐에 넣는다. */
    private class BlePipe(private val socket: BluetoothSocket) : Pipe {
        private val queue = LinkedBlockingQueue<ByteArray>()
        private val output = socket.outputStream

        init {
            Thread({
                val input = socket.inputStream
                val buffer = ByteArray(16 * 1024)
                try {
                    while (true) {
                        val count = input.read(buffer)
                        if (count < 0) break
                        queue.put(buffer.copyOf(count))
                    }
                } catch (error: Exception) {
                    // 닫히면 끝
                }
                queue.put(END)
            }, "sidedesk-ble-read").start()
        }

        override fun receive(): ByteArray? {
            val bytes = queue.poll(POLL_MILLIS, TimeUnit.MILLISECONDS) ?: return ByteArray(0)
            if (bytes === END) {
                queue.put(END)
                return null
            }
            return bytes
        }

        override fun send(bytes: ByteArray) = output.write(bytes)

        override fun close() {
            runCatching { socket.close() }
        }

        private companion object {
            val END = ByteArray(0)
        }
    }

    private companion object {
        const val TAG = "SideDeskBle"
        const val POLL_MILLIS = 250L
        const val READY_LIMIT_NANOS = 2_000_000_000L
    }
}
