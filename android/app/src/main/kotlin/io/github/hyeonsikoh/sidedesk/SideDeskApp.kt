package io.github.hyeonsikoh.sidedesk

import android.app.Application
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ProcessLifecycleOwner
import io.github.hyeonsikoh.sidedesk.ai.PhoneBuzzer
import io.github.hyeonsikoh.sidedesk.clock.PrefsSettings
import io.github.hyeonsikoh.sidedesk.link.ComputerLink
import io.github.hyeonsikoh.sidedesk.rest.Vibration
import io.github.hyeonsikoh.sidedesk.link.KeyFiles
import io.github.hyeonsikoh.sidedesk.link.PhoneLink

class SideDeskApp : Application() {
    val link: ComputerLink by lazy { PhoneLink(this, KeyFiles(filesDir)) }

    /**
     * 세 화면(AI, 단축키, 음악)이 함께 쓰는 진동. 앱에 하나 두어 Activity 를 붙잡지 않는다.
     * 세기와 끔은 쉬기 설정(진동)을 울릴 때마다 설정 파일에서 읽는다.
     */
    val buzzer: PhoneBuzzer by lazy {
        val settings = PrefsSettings(this)
        PhoneBuzzer(this) { Vibration.entries.firstOrNull { it.name == settings.vibration } ?: Vibration.Normal }
    }

    override fun onCreate() {
        super.onCreate()
        ProcessLifecycleOwner.get().lifecycle.addObserver(object : DefaultLifecycleObserver {
            override fun onStart(owner: LifecycleOwner) = link.start()

            override fun onStop(owner: LifecycleOwner) = link.stop()
        })
    }
}
