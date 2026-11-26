package io.github.hyeonsikoh.sidedesk.shots

import android.view.WindowManager
import androidx.activity.ComponentActivity
import io.github.hyeonsikoh.sidedesk.rest.REST_BRIGHTNESS
import io.github.hyeonsikoh.sidedesk.rest.applyWindow
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.rules.RuleChain
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/** 쉼과 켜 두기를 창에 거는 한 곳. 다시 만든 창에도 다시 걸린다. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class RestWindowTest {
    @get:Rule
    val rules: RuleChain = RuleChain.outerRule(registerActivity)

    private fun keepsOn(activity: ComponentActivity) =
        activity.window.attributes.flags and WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON != 0

    @Test
    fun setsBrightnessAndKeepOn() {
        val activity = Robolectric.buildActivity(ComponentActivity::class.java).setup().get()
        activity.applyWindow(resting = true, keepOn = true)
        assertEquals(REST_BRIGHTNESS, activity.window.attributes.screenBrightness)
        assertEquals(true, keepsOn(activity))
        activity.applyWindow(resting = false, keepOn = false)
        assertEquals(WindowManager.LayoutParams.BRIGHTNESS_OVERRIDE_NONE, activity.window.attributes.screenBrightness)
        assertEquals(false, keepsOn(activity))
    }

    @Test
    fun aRecreatedWindowGetsItAgain() {
        val first = Robolectric.buildActivity(ComponentActivity::class.java).setup()
        first.get().applyWindow(resting = true, keepOn = true)
        val again = first.recreate().get()
        // 다시 만든 창은 처음 값이다. onCreate 에서 다시 걸어야 한다.
        assertEquals(false, keepsOn(again))
        again.applyWindow(resting = true, keepOn = true)
        assertEquals(REST_BRIGHTNESS, again.window.attributes.screenBrightness)
        assertEquals(true, keepsOn(again))
    }
}
