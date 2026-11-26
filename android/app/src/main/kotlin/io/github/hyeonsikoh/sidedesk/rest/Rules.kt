package io.github.hyeonsikoh.sidedesk.rest

import io.github.hyeonsikoh.sidedesk.link.AlertKind

/** 앱이 앞에 있을 때 화면을 켜 둘지 */
enum class KeepOn(val label: String) { Always("늘"), Charging("충전 중"), Never("안 함") }

/** 손을 안 댄 뒤 쉴 때까지. null 은 쉬지 않음 */
enum class RestAfter(val label: String, val millis: Long?) {
    One("1분", 60_000),
    Five("5분", 300_000),
    Fifteen("15분", 900_000),
    Never("안 함", null),
}

/** 알림과 버튼 누름 진동 모두에 걸린다. amplitude 0 은 끔 */
enum class Vibration(val label: String, val amplitude: Int) {
    Off("끔", 0),
    Weak("약하게", 80),
    Normal("보통", 160),
    Strong("강하게", 255),
}

/** "늘 켜 두기"여도 충전 중이 아니고 이보다 낮으면 풂 */
const val LOW_BATTERY = 15

/** 끈적 배터리 방송 값에서 충전 여부. 가득 찬 채 꽂혀 있어도 충전 중으로 본다. */
fun charging(plugged: Int): Boolean = plugged != 0

/** level 과 scale 에서 퍼센트. 값이 없으면 null */
fun percent(level: Int, scale: Int): Int? = if (level < 0 || scale <= 0) null else level * 100 / scale

fun keepScreenOn(setting: KeepOn, charging: Boolean, percent: Int?): Boolean = when (setting) {
    KeepOn.Always -> charging || (percent ?: 100) >= LOW_BATTERY
    KeepOn.Charging -> charging
    KeepOn.Never -> false
}

/** 번인을 막는 시계 옮김(dp). 1분마다 가운데 둘레 ±12dp 안의 9자리를 차례로 돈다. */
fun shiftAt(minute: Long): Pair<Int, Int> {
    val spots = listOf(0 to 0, 12 to 0, 12 to 12, 0 to 12, -12 to 12, -12 to 0, -12 to -12, 0 to -12, 12 to -12)
    return spots[Math.floorMod(minute, spots.size.toLong()).toInt()]
}

/** 진동 모양. 쉼 칸은 진폭 0. 끔이면 null */
fun waveform(kind: AlertKind?, vibration: Vibration): Pair<LongArray, IntArray>? {
    if (vibration == Vibration.Off) return null
    val on = vibration.amplitude
    return when (kind) {
        null -> longArrayOf(0, 20) to intArrayOf(0, on)
        AlertKind.Waiting -> longArrayOf(0, 60, 120, 60) to intArrayOf(0, on, 0, on)
        else -> longArrayOf(0, 80) to intArrayOf(0, on)
    }
}
