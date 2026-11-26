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

    fun allowed() = ContextCompat.checkSelfPermission(context, Manifest.permission.ACCESS_COARSE_LOCATION) ==
        PackageManager.PERMISSION_GRANTED

    /** 권한이 없으면 위치 없음. 위치 기능이 꺼졌거나 못 얻으면 그대로 둔다. */
    @SuppressLint("MissingPermission")
    fun refresh() {
        if (!allowed()) {
            link.setLocation(LocationState.Off)
            return
        }
        if (manager == null || !manager.isProviderEnabled(LocationManager.NETWORK_PROVIDER)) return
        stop()
        val signal = CancellationSignal().also { cancel = it }
        // 취소하면 아래 콜백이 오지 않으므로, 시간이 넘으면 여기서 마지막 위치로 대신한다.
        val timeout = Runnable {
            signal.cancel()
            use(null)
        }
        pending = timeout
        main.postDelayed(timeout, TIMEOUT_MILLIS)
        val run = Executor { main.post(it) }
        try {
            LocationManagerCompat.getCurrentLocation(manager, LocationManager.NETWORK_PROVIDER, signal, run) { found: Location? ->
                main.removeCallbacks(timeout)
                use(found)
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
            found ?: manager?.getLastKnownLocation(LocationManager.NETWORK_PROVIDER)
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
