package io.github.hyeonsikoh.sidedesk.music

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class MusicLayoutTest {
    @Test fun `정사각에 가까운 큰 판은 쌓음`() = assertTrue(musicLayout(600f, 560f).stacked)

    @Test fun `바형 가로는 옆으로`() = assertFalse(musicLayout(560f, 360f).stacked)

    @Test fun `세로는 쌓음`() = assertTrue(musicLayout(400f, 600f).stacked)

    @Test fun `낮으면 그림 34퍼센트`() = assertEquals(500f * 0.34f, musicLayout(500f, 300f).art, 0.01f)
}
