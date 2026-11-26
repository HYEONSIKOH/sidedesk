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
        val timeout = Runnable { signal.cancel() }
        main.postDelayed(timeout, TIMEOUT_MILLIS)
        val run = Executor { main.post(it) }
        LocationManagerCompat.getCurrentLocation(manager, LocationManager.NETWORK_PROVIDER, signal, run) { found: Location? ->
            main.removeCallbacks(timeout)
            val place = found ?: manager.getLastKnownLocation(LocationManager.NETWORK_PROVIDER)
            if (place != null) link.setLocation(LocationState.At(place.latitude, place.longitude))
        }
    }

    fun stop() {
        cancel?.cancel()
        cancel = null
    }

    private companion object {
        const val TIMEOUT_MILLIS = 30_000L
    }
}
