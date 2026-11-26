package io.github.hyeonsikoh.sidedesk

import android.app.Application
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ProcessLifecycleOwner
import io.github.hyeonsikoh.sidedesk.link.ComputerLink
import io.github.hyeonsikoh.sidedesk.link.KeyFiles
import io.github.hyeonsikoh.sidedesk.link.PhoneLink

class SideDeskApp : Application() {
    val link: ComputerLink by lazy { PhoneLink(KeyFiles(filesDir)) }

    override fun onCreate() {
        super.onCreate()
        ProcessLifecycleOwner.get().lifecycle.addObserver(object : DefaultLifecycleObserver {
            override fun onStart(owner: LifecycleOwner) = link.start()

            override fun onStop(owner: LifecycleOwner) = link.stop()
        })
    }
}
