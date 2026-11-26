package io.github.hyeonsikoh.sidedesk.shots

import android.content.ComponentName
import android.content.Context
import androidx.activity.ComponentActivity
import androidx.test.core.app.ApplicationProvider
import org.junit.rules.TestRule
import org.junit.runners.model.Statement
import org.robolectric.Shadows.shadowOf

/** 시험용 빈 화면을 Robolectric 에만 등록한다. 앱 매니페스트와 APK 는 건드리지 않는다. */
val registerActivity = TestRule { base, _ ->
    object : Statement() {
        override fun evaluate() {
            val context = ApplicationProvider.getApplicationContext<Context>()
            shadowOf(context.packageManager).addActivityIfNotPresent(ComponentName(context, ComponentActivity::class.java))
            base.evaluate()
        }
    }
}
