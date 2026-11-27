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

/** 런처 아이콘을 찍는다: 적응형 아이콘 전체, 앞 층. 결과는 screenshots/icon/ */
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
        save("foreground", draw(size) { c -> c.drawColor(0xFF22272E.toInt()); icon.foreground.setBounds(0, 0, size, size); icon.foreground.draw(c) })
        // 단색 층이 없어야 테마 아이콘을 켠 폰에서도 색이 그대로 나온다.
        assertTrue(icon.monochrome == null)
    }
}
