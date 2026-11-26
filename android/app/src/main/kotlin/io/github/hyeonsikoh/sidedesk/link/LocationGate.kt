package io.github.hyeonsikoh.sidedesk.link

/**
 * 한 연결 안에서 위치를 언제 보낼지 정한다. 접속마다 새로 만든다.
 * 모르면 보내지 않고, 위치 없음은 null 로, 같은 상태는 한 번만 보낸다.
 */
class LocationGate {
    private var sent: LocationState = LocationState.Unknown

    /** 보낼 것이 있으면 그 상태, 없으면 null */
    fun take(now: LocationState): LocationState? {
        if (now == LocationState.Unknown || now == sent) return null
        sent = now
        return now
    }
}
