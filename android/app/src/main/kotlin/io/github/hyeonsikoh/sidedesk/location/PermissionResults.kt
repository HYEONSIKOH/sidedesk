package io.github.hyeonsikoh.sidedesk.location

import android.Manifest
import android.content.pm.PackageManager

/** 한 번에 받은 권한 결과를 권한마다 따로 본다. */
data class PermissionResults(
    /** 블루투스 둘이 허용돼 블루투스를 다시 열어야 한다 */
    val restartBluetooth: Boolean,
    /** 위치를 물었으면 허용 여부, 묻지 않았으면 null */
    val location: Boolean?,
) {
    companion object {
        val BLUETOOTH = listOf(Manifest.permission.BLUETOOTH_ADVERTISE, Manifest.permission.BLUETOOTH_CONNECT)
        const val LOCATION = Manifest.permission.ACCESS_COARSE_LOCATION
        /** GPS 를 쓰려면 필요하다. 같이 물으면 사람이 대략/정확을 고른다. */
        const val PRECISE = Manifest.permission.ACCESS_FINE_LOCATION

        fun of(permissions: Array<String>, grants: IntArray): PermissionResults {
            val granted = permissions.indices.associate { permissions[it] to (grants.getOrNull(it) == PackageManager.PERMISSION_GRANTED) }
            val askedBluetooth = BLUETOOTH.any { it in granted }
            return PermissionResults(
                restartBluetooth = askedBluetooth && BLUETOOTH.filter { it in granted }.all { granted[it] == true },
                location = granted[LOCATION] ?: granted[PRECISE],
            )
        }
    }
}
