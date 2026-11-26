package io.github.hyeonsikoh.sidedesk.rest

import android.app.Activity
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.BatteryManager
import android.view.WindowManager
import androidx.core.content.ContextCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat

/** 쉬는 동안 창 밝기. 시스템 밝기 설정은 바꾸지 않는다. */
const val REST_BRIGHTNESS = 0.05f

/**
 * 쉼과 켜 두기 결과를 창에 거는 한 곳.
 * 접고 펴서 창이 새로 만들어지거나 테마 때문에 바 설정을 다시 걸어도, 여기를 다시 불러 맞춘다.
 */
fun Activity.applyWindow(resting: Boolean, keepOn: Boolean) {
    if (keepOn) window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON) else window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
    window.attributes = window.attributes.apply {
        screenBrightness = if (resting) REST_BRIGHTNESS else WindowManager.LayoutParams.BRIGHTNESS_OVERRIDE_NONE
    }
    val bars = WindowCompat.getInsetsController(window, window.decorView)
    if (resting) {
        bars.systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
        bars.hide(WindowInsetsCompat.Type.systemBars())
    } else {
        bars.show(WindowInsetsCompat.Type.systemBars())
    }
}

/** 충전 여부와 배터리 퍼센트를 본다. 앱이 앞에 있는 동안만 등록한다. */
class BatteryWatch(private val context: Context, private val onChange: (charging: Boolean, percent: Int?) -> Unit) {
    private val receiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) = read(intent)
    }
    private var registered = false

    fun start() {
        if (registered) return
        registered = true
        // 끈적 방송이라 등록하는 순간 지금 값이 함께 온다.
        ContextCompat.registerReceiver(context, receiver, IntentFilter(Intent.ACTION_BATTERY_CHANGED), ContextCompat.RECEIVER_NOT_EXPORTED)
            ?.let(::read)
    }

    fun stop() {
        if (!registered) return
        registered = false
        context.unregisterReceiver(receiver)
    }

    private fun read(intent: Intent) = onChange(
        charging(intent.getIntExtra(BatteryManager.EXTRA_PLUGGED, 0)),
        percent(intent.getIntExtra(BatteryManager.EXTRA_LEVEL, -1), intent.getIntExtra(BatteryManager.EXTRA_SCALE, -1)),
    )
}
