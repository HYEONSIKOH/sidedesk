package io.github.hyeonsikoh.sidedesk.ai

import android.content.Context
import android.os.Build
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager
import io.github.hyeonsikoh.sidedesk.link.AlertKind
import io.github.hyeonsikoh.sidedesk.rest.Vibration
import io.github.hyeonsikoh.sidedesk.rest.waveform

/** 알림 진동. 시험에서는 가짜를 쓴다. */
fun interface Buzzer {
    fun buzz(kind: AlertKind)

    /** 버튼을 눌렀을 때 짧게 한 번 */
    fun tap() {}
}

/** 끝남은 짧게 한 번, 답변 대기는 짧게 두 번. 세기와 끔은 쉬기 설정(진동)을 따른다. */
class PhoneBuzzer(context: Context, private val vibration: () -> Vibration = { Vibration.Normal }) : Buzzer {
    private val vibrator: Vibrator? = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
        context.getSystemService(VibratorManager::class.java)?.defaultVibrator
    } else {
        @Suppress("DEPRECATION")
        context.getSystemService(Vibrator::class.java)
    }

    /** 세기를 바꿀 수 있는 폰인지. 아니면 설정에 안내를 보인다. */
    val canSetStrength: Boolean get() = vibrator?.hasAmplitudeControl() == true

    override fun buzz(kind: AlertKind) = play(kind)

    override fun tap() = play(null)

    private fun play(kind: AlertKind?) {
        val target = vibrator?.takeIf { it.hasVibrator() } ?: return
        val (timings, amplitudes) = waveform(kind, vibration()) ?: return
        val effect = if (target.hasAmplitudeControl()) {
            VibrationEffect.createWaveform(timings, amplitudes, -1)
        } else {
            VibrationEffect.createWaveform(timings, -1)
        }
        target.vibrate(effect)
    }
}
