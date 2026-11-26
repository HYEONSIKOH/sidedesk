package io.github.hyeonsikoh.sidedesk.ui

import androidx.compose.runtime.Composable
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.sp

/** 판 크기에 맞춘 글자 크기. 글자 배율로 나눠, 배율을 키워도 판을 넘지 않게 한다. */
@Composable
fun fit(size: Dp): TextUnit = with(LocalDensity.current) { (size.value / fontScale).sp }
