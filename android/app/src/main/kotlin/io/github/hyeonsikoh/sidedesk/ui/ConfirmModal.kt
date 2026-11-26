package io.github.hyeonsikoh.sidedesk.ui

import android.os.Build
import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.scaleOut
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.blur
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/** 확인 창에 띄울 내용. 앱의 모든 확인 창은 이 모양 하나를 쓴다. */
data class Confirm(
    val title: String,
    /** 회색 설명, 두 줄까지 */
    val description: String,
    val confirmLabel: String,
    val cancelLabel: String = "취소",
    /** 되돌릴 수 없는 일이면 빨갛게 */
    val danger: Boolean = false,
    /** 위 동그라미에 넣을 글자 하나 */
    val glyph: String = "!",
)

private const val MILLIS = 180

/** 확인 창이 떠 있을 때 뒤 화면을 흐리게 한다. Android 12 아래에서는 어둡게만 한다. */
@Composable
fun Modifier.behindModal(open: Boolean): Modifier {
    val radius by animateDpAsState(if (open) 16.dp else 0.dp, tween(MILLIS), label = "blur")
    return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S && radius > 0.dp) blur(radius) else this
}

/** 화면 맨 위에 겹쳐 그린다. 판 밖을 누르거나 뒤로 가면 취소다. */
@Composable
fun BoxScope.ConfirmModal(confirm: Confirm?, onConfirm: () -> Unit, onDismiss: () -> Unit) {
    // 사라지는 동안에도 마지막 내용을 보인다.
    val last = remember { arrayOfNulls<Confirm>(1) }
    if (confirm != null) last[0] = confirm
    BackHandler(enabled = confirm != null, onBack = onDismiss)
    AnimatedVisibility(
        visible = confirm != null,
        enter = fadeIn(tween(MILLIS)),
        exit = fadeOut(tween(MILLIS)),
        modifier = Modifier.matchParentSize(),
    ) {
        Box(
            Modifier
                .fillMaxSize()
                .background(Color.Black.copy(alpha = 0.6f))
                .clickable(remember { MutableInteractionSource() }, indication = null, onClick = onDismiss),
            contentAlignment = Alignment.Center,
        ) {
            val shown = last[0] ?: return@Box
            Panel(
                shown,
                onConfirm,
                onDismiss,
                Modifier.animateEnterExit(
                    enter = scaleIn(tween(MILLIS), initialScale = 0.92f),
                    exit = scaleOut(tween(MILLIS), targetScale = 0.92f),
                ),
            )
        }
    }
}

@Composable
private fun Panel(confirm: Confirm, onConfirm: () -> Unit, onDismiss: () -> Unit, modifier: Modifier) {
    val shape = RoundedCornerShape(28.dp)
    Column(
        modifier
            .padding(24.dp)
            .widthIn(max = 400.dp)
            .fillMaxWidth()
            .clip(shape)
            .background(Palette.raised)
            .border(1.dp, Color.White.copy(alpha = 0.06f), shape)
            // 판 안을 누르면 닫히지 않게 막는다.
            .clickable(remember { MutableInteractionSource() }, indication = null) {}
            .padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        val tint = if (confirm.danger) Palette.danger else Palette.accent
        Box(
            Modifier.size(56.dp).clip(CircleShape).background(tint.copy(alpha = 0.18f)),
            contentAlignment = Alignment.Center,
        ) {
            Text(confirm.glyph, color = tint, fontSize = 22.sp, fontWeight = FontWeight.SemiBold)
        }
        Text(
            confirm.title,
            color = Palette.ink,
            fontSize = 20.sp,
            fontWeight = FontWeight.Bold,
            textAlign = TextAlign.Center,
            modifier = Modifier.padding(top = 16.dp),
        )
        Text(
            confirm.description,
            color = Palette.muted,
            fontSize = 14.sp,
            lineHeight = 20.sp,
            maxLines = 2,
            textAlign = TextAlign.Center,
            modifier = Modifier.padding(top = 8.dp),
        )
        Row(Modifier.padding(top = 24.dp).fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            Pill(confirm.cancelLabel, Palette.card, Palette.ink, Modifier.weight(1f), onDismiss)
            Pill(confirm.confirmLabel, tint, if (confirm.danger) Color.White else Palette.ground, Modifier.weight(1f), onConfirm)
        }
    }
}

@Composable
private fun Pill(label: String, background: Color, ink: Color, modifier: Modifier, onClick: () -> Unit) {
    Box(
        modifier.height(PILL).clip(CircleShape).background(background).clickable(onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        Text(label, color = ink, fontSize = 16.sp, fontWeight = FontWeight.SemiBold)
    }
}

private val PILL: Dp = 48.dp
