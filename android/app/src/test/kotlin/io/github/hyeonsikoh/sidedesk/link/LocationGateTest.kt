package io.github.hyeonsikoh.sidedesk.link

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class LocationGateTest {
    private val seoul = LocationState.At(37.56, 126.97)

    @Test
    fun sendsNothingWhileUnknown() {
        assertNull(LocationGate().take(LocationState.Unknown))
    }

    @Test
    fun sendsOffAsNoLocation() {
        assertEquals(LocationState.Off, LocationGate().take(LocationState.Off))
    }

    @Test
    fun sendsTheSameStateOnce() {
        val gate = LocationGate()
        assertEquals(seoul, gate.take(seoul))
        assertNull(gate.take(seoul))
        assertEquals(LocationState.Off, gate.take(LocationState.Off))
        assertEquals(seoul, gate.take(seoul))
    }

    @Test
    fun aNewConnectionSendsAgain() {
        LocationGate().take(seoul)
        assertEquals(seoul, LocationGate().take(seoul))
    }

    @Test
    fun goingBackToUnknownKeepsTheLastSent() {
        val gate = LocationGate()
        gate.take(seoul)
        assertNull(gate.take(LocationState.Unknown))
        assertNull(gate.take(seoul))
    }
}
