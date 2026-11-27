package io.github.hyeonsikoh.sidedesk.location

import android.Manifest
import android.annotation.SuppressLint
import android.content.Context
import android.content.pm.PackageManager
import android.location.Location
import android.location.LocationManager
import android.os.Handler
import android.os.Looper
import androidx.core.content.ContextCompat
import androidx.core.location.LocationManagerCompat
import androidx.core.os.CancellationSignal
import io.github.hyeonsikoh.sidedesk.link.ComputerLink
import io.github.hyeonsikoh.sidedesk.link.LocationState
import java.util.concurrent.Executor

/** 앱이 앞에 올 때 대략적인 위치를 한 번 읽어 컴퓨터로 넘긴다. 반올림은 core 가 한다. */
class CoarseLocation(private val context: Context, private val link: ComputerLink) {
    private val manager = context.getSystemService(LocationManager::class.java)
    private val main = Handler(Looper.getMainLooper())
    private var cancel: CancellationSignal? = null
    private var pending: Runnable? = null

    fun allowed() = has(Manifest.permission.ACCESS_COARSE_LOCATION) || has(Manifest.permission.ACCESS_FINE_LOCATION)

    private fun has(permission: String) = ContextCompat.checkSelfPermission(context, permission) == PackageManager.PERMISSION_GRANTED

    /** 읽을 곳을 차례대로. 와이파이, 기지국이 먼저(빠름), 그다음 GPS. */
    private fun providers(): List<String> {
        val manager = manager ?: return emptyList()
        val enabled = { name: String -> runCatching { manager.isProviderEnabled(name) }.getOrDefault(false) }
        return chooseProviders(
            network = enabled(LocationManager.NETWORK_PROVIDER),
            gps = enabled(LocationManager.GPS_PROVIDER),
            precise = has(Manifest.permission.ACCESS_FINE_LOCATION),
        )
    }

    /** 권한이 없으면 위치 없음. 위치 기능이 꺼졌거나 못 얻으면 그대로 둔다. */
    @SuppressLint("MissingPermission")
    fun refresh() {
        if (!allowed()) {
            link.setLocation(LocationState.Off)
            return
        }
        val providers = providers()
        if (providers.isEmpty()) return
        stop()
        read(providers)
    }

    /** 앞의 곳에서 못 얻으면(비행기 모드의 기지국 등) 다음 곳에서 다시 읽는다. */
    @SuppressLint("MissingPermission")
    private fun read(providers: List<String>) {
        val signal = CancellationSignal().also { cancel = it }
        val rest = providers.drop(1)
        val next = { found: Location? -> if (found == null && rest.isNotEmpty()) read(rest) else use(found) }
        // 취소하면 아래 콜백이 오지 않으므로, 시간이 넘으면 여기서 대신 넘어간다.
        val timeout = Runnable {
            signal.cancel()
            next(null)
        }
        pending = timeout
        main.postDelayed(timeout, TIMEOUT_MILLIS)
        val run = Executor { main.post(it) }
        try {
            LocationManagerCompat.getCurrentLocation(manager!!, providers.first(), signal, run) { found: Location? ->
                main.removeCallbacks(timeout)
                next(found)
            }
        } catch (error: SecurityException) {
            // 확인과 요청 사이에 권한을 거둔 경우
            main.removeCallbacks(timeout)
            link.setLocation(LocationState.Off)
        }
    }

    /** 받은 위치, 없으면 마지막 위치를 넘긴다. 둘 다 없으면 그대로 둔다. */
    @SuppressLint("MissingPermission")
    private fun use(found: Location?) {
        val place = try {
            found ?: listOf(LocationManager.NETWORK_PROVIDER, LocationManager.GPS_PROVIDER)
                .mapNotNull { name -> runCatching { manager?.getLastKnownLocation(name) }.getOrNull() }
                .maxByOrNull { it.time }
        } catch (error: SecurityException) {
            link.setLocation(LocationState.Off)
            return
        }
        if (place != null) link.setLocation(LocationState.At(place.latitude, place.longitude))
    }

    fun stop() {
        cancel?.cancel()
        cancel = null
        pending?.let(main::removeCallbacks)
        pending = null
    }

    private companion object {
        const val TIMEOUT_MILLIS = 30_000L
    }
}

/** 와이파이, 기지국이 켜져 있으면 그쪽 먼저, 정확한 위치 권한이 있으면 GPS 를 뒤에. */
internal fun chooseProviders(network: Boolean, gps: Boolean, precise: Boolean): List<String> = buildList {
    if (network) add(LocationManager.NETWORK_PROVIDER)
    if (gps && precise) add(LocationManager.GPS_PROVIDER)
}
