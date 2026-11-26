package io.github.hyeonsikoh.sidedesk.ai

import android.content.Context
import android.os.Build
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager
import io.github.hyeonsikoh.sidedesk.link.AlertKind

/** 알림 진동. 시험에서는 가짜를 쓴다. */
fun interface Buzzer {
    fun buzz(kind: AlertKind)

    /** 버튼을 눌렀을 때 짧게 한 번 */
    fun tap() {}
}

/** 끝남은 짧게 한 번, 답변 대기는 짧게 두 번. 세기는 기기 기본값이다. */
class PhoneBuzzer(context: Context) : Buzzer {
    private val vibrator: Vibrator? = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
        context.getSystemService(VibratorManager::class.java)?.defaultVibrator
    } else {
        @Suppress("DEPRECATION")
        context.getSystemService(Vibrator::class.java)
    }

    override fun buzz(kind: AlertKind) {
        val target = vibrator?.takeIf { it.hasVibrator() } ?: return
        val pattern = if (kind == AlertKind.Waiting) longArrayOf(0, 60, 120, 60) else longArrayOf(0, 80)
        target.vibrate(VibrationEffect.createWaveform(pattern, -1))
    }

    override fun tap() {
        val target = vibrator?.takeIf { it.hasVibrator() } ?: return
        target.vibrate(VibrationEffect.createOneShot(20, VibrationEffect.DEFAULT_AMPLITUDE))
    }
}
