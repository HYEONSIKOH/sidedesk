package io.github.hyeonsikoh.sidedesk.ui

import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.ReadOnlyComposable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color

enum class Theme { Light, Dark }

/** 앱 색 한 벌. docs/images 의 screens, hero 그림에서 뽑았다. */
data class Colors(
    val ground: Color,
    val panel: Color,
    val card: Color,
    val raised: Color,
    val side: Color,
    val liquid: Color,
    val accent: Color,
    val ink: Color,
    val muted: Color,
    val danger: Color,
    /** 일이 있는 작은 칸과 작업 중 카드 바탕(hero 의 AI 칸) */
    val hot: Color,
    /** 답변 대기처럼 사람 손이 필요한 것 */
    val waiting: Color,
    /** 시계 숫자 위아래 그라데이션 */
    val digitTop: Color,
    val digitBottom: Color,
    val dark: Boolean,
)

val DarkColors = Colors(
    ground = Color(0xFF15181C),
    panel = Color(0xFF22272E),
    card = Color(0xFF2C323A),
    raised = Color(0xFF353C45),
    side = Color(0xFF1D2127),
    liquid = Color(0xFF33475A),
    accent = Color(0xFF8FAECB),
    ink = Color(0xFFDCE1E7),
    muted = Color(0xFF8B939E),
    danger = Color(0xFFE5484D),
    hot = Color(0xFF2B3744),
    waiting = Color(0xFFE0A458),
    digitTop = Color(0xFFD3DEE9),
    digitBottom = Color(0xFF7D93AB),
    dark = true,
)

val LightColors = Colors(
    ground = Color(0xFFE6E9ED),
    panel = Color(0xFFF6F7F9),
    card = Color(0xFFE5E9EE),
    raised = Color(0xFFFFFFFF),
    side = Color(0xFFF1F3F6),
    liquid = Color(0xFFCBD8E4),
    accent = Color(0xFF4F7090),
    ink = Color(0xFF2C333C),
    muted = Color(0xFF6E7783),
    danger = Color(0xFFD93D42),
    hot = Color(0xFFDCE4EC),
    waiting = Color(0xFFB7791F),
    digitTop = Color(0xFF7F97B0),
    digitBottom = Color(0xFFAABCCE),
    dark = false,
)

val LocalPalette = staticCompositionLocalOf { DarkColors }

/** 지금 테마의 색 */
val Palette: Colors
    @Composable @ReadOnlyComposable get() = LocalPalette.current

@Composable
fun SideDeskTheme(theme: Theme, content: @Composable () -> Unit) {
    CompositionLocalProvider(LocalPalette provides if (theme == Theme.Light) LightColors else DarkColors, content = content)
}
