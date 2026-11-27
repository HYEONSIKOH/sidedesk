package io.github.hyeonsikoh.sidedesk.desk

import io.github.hyeonsikoh.sidedesk.ui.zoomFor
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class ZoomTest {
    @Test fun `바형 가로가 기준`() = assertEquals(1f, zoomFor(914f, 411f), 0.001f)

    @Test fun `작은 폰은 줄이지 않음`() = assertEquals(1f, zoomFor(344f, 882f), 0f)

    @Test fun `태블릿은 1점6배까지`() = assertEquals(1.6f, zoomFor(1280f, 800f), 0f)

    @Test fun `펼친 폴드는 조금 키움`() = assertTrue(zoomFor(841f, 673f) > 1.2f)
}
