package io.github.hyeonsikoh.sidedesk.ui

import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Density
import kotlin.math.sqrt

private val BASE = sqrt(914f * 411f)

/** 화면 배율. 바형 폰 가로(914x411dp)를 1로 두고 넓이에 비례해 1~1.6배. */
fun zoomFor(widthDp: Float, heightDp: Float): Float =
    (sqrt(widthDp.coerceAtLeast(0f) * heightDp.coerceAtLeast(0f)) / BASE).coerceIn(1f, 1.6f)

/** 큰 화면일수록 화면 전체(글자, 칸)를 같은 배율로 키운다. */
@Composable
fun Zoomed(content: @Composable () -> Unit) {
    BoxWithConstraints(Modifier.fillMaxSize()) {
        val zoom = zoomFor(maxWidth.value, maxHeight.value)
        val density = LocalDensity.current
        CompositionLocalProvider(LocalDensity provides Density(density.density * zoom, density.fontScale)) { content() }
    }
}
