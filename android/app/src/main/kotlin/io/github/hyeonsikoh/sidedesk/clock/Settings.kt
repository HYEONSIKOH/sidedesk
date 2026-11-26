package io.github.hyeonsikoh.sidedesk.clock

import android.content.Context

/** 기기에 남기는 고른 값. 앱을 껐다 켜도 남는다. */
interface Settings {
    var face: String?
    var theme: String?
    /** 위치 안내를 한 번 보였는지 */
    var askedLocation: Boolean
}

class PrefsSettings(context: Context) : Settings {
    private val prefs = context.getSharedPreferences("settings", Context.MODE_PRIVATE)

    override var face: String?
        get() = prefs.getString("face", null)
        set(value) = prefs.edit().putString("face", value).apply()

    override var theme: String?
        get() = prefs.getString("theme", null)
        set(value) = prefs.edit().putString("theme", value).apply()

    override var askedLocation: Boolean
        get() = prefs.getBoolean("askedLocation", false)
        set(value) = prefs.edit().putBoolean("askedLocation", value).apply()
}
