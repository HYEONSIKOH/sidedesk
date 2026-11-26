package io.github.hyeonsikoh.sidedesk.rest

import io.github.hyeonsikoh.sidedesk.link.AlertKind
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import kotlin.math.abs

class RestRulesTest {
    @Test
    fun keepsTheScreenOnByTheSetting() {
        assertEquals(true, keepScreenOn(KeepOn.Always, charging = false, percent = 15))
        assertEquals(true, keepScreenOn(KeepOn.Always, charging = false, percent = 16))
        assertEquals(false, keepScreenOn(KeepOn.Always, charging = false, percent = 14))
        assertEquals(true, keepScreenOn(KeepOn.Always, charging = true, percent = 3))
        assertEquals(true, keepScreenOn(KeepOn.Always, charging = false, percent = null))
        assertEquals(true, keepScreenOn(KeepOn.Charging, charging = true, percent = 100))
        assertEquals(false, keepScreenOn(KeepOn.Charging, charging = false, percent = 100))
        assertEquals(false, keepScreenOn(KeepOn.Never, charging = true, percent = 100))
    }

    @Test
    fun readsTheBattery() {
        // 가득 찬 채 꽂혀 있어도 EXTRA_PLUGGED 가 0 이 아니라 충전 중이다.
        assertTrue(charging(plugged = 1))
        assertEquals(false, charging(plugged = 0))
        assertEquals(15, percent(level = 30, scale = 200))
        assertEquals(100, percent(level = 100, scale = 100))
        assertNull(percent(level = -1, scale = 100))
        assertNull(percent(level = 50, scale = 0))
    }

    @Test
    fun shiftsAroundNineSpotsWithinTwelveDp() {
        val spots = (0L until 9L).map(::shiftAt)
        assertEquals(9, spots.toSet().size)
        spots.forEach { (x, y) -> assertTrue(abs(x) <= 12 && abs(y) <= 12) }
        assertEquals(shiftAt(0), shiftAt(9))
        assertEquals(shiftAt(8), shiftAt(-1))
    }

    @Test
    fun shapesTheVibration() {
        assertNull(waveform(AlertKind.Done, Vibration.Off))
        assertNull(waveform(null, Vibration.Off))
        val (timings, amplitudes) = waveform(AlertKind.Waiting, Vibration.Weak)!!
        assertArrayEquals(longArrayOf(0, 60, 120, 60), timings)
        assertArrayEquals(intArrayOf(0, 80, 0, 80), amplitudes)
        assertArrayEquals(intArrayOf(0, 255), waveform(AlertKind.Done, Vibration.Strong)!!.second)
        assertArrayEquals(intArrayOf(0, 160), waveform(null, Vibration.Normal)!!.second)
    }
}
