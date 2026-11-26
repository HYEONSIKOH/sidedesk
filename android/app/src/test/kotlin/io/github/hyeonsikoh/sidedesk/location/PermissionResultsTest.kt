package io.github.hyeonsikoh.sidedesk.location

import android.content.pm.PackageManager.PERMISSION_DENIED
import android.content.pm.PackageManager.PERMISSION_GRANTED
import org.junit.Assert.assertEquals
import org.junit.Test

class PermissionResultsTest {
    private val advertise = PermissionResults.BLUETOOTH[0]
    private val connect = PermissionResults.BLUETOOTH[1]
    private val location = PermissionResults.LOCATION

    private fun of(vararg pairs: Pair<String, Int>) =
        PermissionResults.of(pairs.map { it.first }.toTypedArray(), pairs.map { it.second }.toIntArray())

    @Test
    fun bluetoothOnlyRestartsEvenIfLocationIsDenied() {
        assertEquals(
            PermissionResults(restartBluetooth = true, location = false),
            of(advertise to PERMISSION_GRANTED, connect to PERMISSION_GRANTED, location to PERMISSION_DENIED),
        )
    }

    @Test
    fun locationOnly() {
        assertEquals(
            PermissionResults(restartBluetooth = false, location = true),
            of(advertise to PERMISSION_DENIED, connect to PERMISSION_GRANTED, location to PERMISSION_GRANTED),
        )
    }

    @Test
    fun bothDenied() {
        assertEquals(
            PermissionResults(restartBluetooth = false, location = false),
            of(advertise to PERMISSION_DENIED, connect to PERMISSION_DENIED, location to PERMISSION_DENIED),
        )
    }

    @Test
    fun oneMissingBluetoothPermissionGranted() {
        assertEquals(PermissionResults(restartBluetooth = true, location = null), of(connect to PERMISSION_GRANTED))
    }

    @Test
    fun onlyLocationAskedOnOlderAndroid() {
        assertEquals(PermissionResults(restartBluetooth = false, location = true), of(location to PERMISSION_GRANTED))
    }

    @Test
    fun cancelledRequestHasNoResults() {
        assertEquals(PermissionResults(restartBluetooth = false, location = null), PermissionResults.of(emptyArray(), IntArray(0)))
    }
}
