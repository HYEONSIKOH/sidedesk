package io.github.hyeonsikoh.sidedesk.shots

import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.drawable.AdaptiveIconDrawable
import androidx.test.core.app.ApplicationProvider
import io.github.hyeonsikoh.sidedesk.R
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.io.File

/** 런처 아이콘을 찍는다: 적응형 아이콘 전체, 앞 층, 단색 층. 결과는 screenshots/icon/ */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [34])
class IconShots {
    private val context = ApplicationProvider.getApplicationContext<android.content.Context>()

    private fun save(name: String, bitmap: Bitmap) {
        val dir = File("screenshots/icon").apply { mkdirs() }
        File(dir, "$name.png").outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
    }

    private fun draw(size: Int, block: (Canvas) -> Unit) = Bitmap.createBitmap(size, size, Bitmap.Config.ARGB_8888).also { block(Canvas(it)) }

    @Test
    fun launcherIcon() {
        val icon = context.getDrawable(R.mipmap.ic_launcher) as AdaptiveIconDrawable
        val size = 432
        val whole = draw(size) { icon.setBounds(0, 0, size, size); icon.draw(it) }
        save("launcher", whole)
        // 가운데(폰 테두리)는 밝은 글자색, 모서리 밖은 비어 있거나 바탕색이어야 한다.
        assertTrue(Color.alpha(whole.getPixel(size / 2, size / 2)) > 0)
        for ((name, layer) in listOf("foreground" to icon.foreground, "monochrome" to icon.monochrome!!)) {
            save(name, draw(size) { c -> c.drawColor(if (name == "monochrome") Color.DKGRAY else 0xFF22272E.toInt()); layer.setBounds(0, 0, size, size); layer.draw(c) })
        }
    }
}
