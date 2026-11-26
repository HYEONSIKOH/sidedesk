package io.github.hyeonsikoh.sidedesk.ui

import androidx.compose.runtime.Composable
import androidx.compose.runtime.NonRestartableComposable
import androidx.compose.runtime.NonSkippableComposable
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.staticCompositionLocalOf

/** 시험에서만 넣는다. 칸 이름을 받아 다시 구성된 횟수를 센다. 앱에서는 null 이라 아무 일도 하지 않는다. */
val LocalTrack = staticCompositionLocalOf<((String) -> Unit)?> { null }

/** 이 자리가 다시 구성될 때마다 시험에 알린다. 부른 칸과 함께 다시 구성되도록 건너뛰지 않는다. */
@NonSkippableComposable
@NonRestartableComposable
@Composable
fun Track(name: String) {
    val track = LocalTrack.current ?: return
    SideEffect { track(name) }
}
