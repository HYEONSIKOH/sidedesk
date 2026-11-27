package io.github.hyeonsikoh.sidedesk.location

import android.location.LocationManager.GPS_PROVIDER
import android.location.LocationManager.NETWORK_PROVIDER
import org.junit.Assert.assertEquals
import org.junit.Test

class ChooseProviderTest {
    @Test fun `와이파이 기지국 먼저 그다음 GPS`() =
        assertEquals(listOf(NETWORK_PROVIDER, GPS_PROVIDER), chooseProviders(network = true, gps = true, precise = true))

    @Test fun `기지국이 꺼져 있으면 GPS 만`() =
        assertEquals(listOf(GPS_PROVIDER), chooseProviders(network = false, gps = true, precise = true))

    @Test fun `대략 위치 권한만 있으면 GPS 를 못 씀`() =
        assertEquals(listOf(NETWORK_PROVIDER), chooseProviders(network = true, gps = true, precise = false))

    @Test fun `읽을 곳이 없음`() =
        assertEquals(emptyList<String>(), chooseProviders(network = false, gps = false, precise = true))
}
