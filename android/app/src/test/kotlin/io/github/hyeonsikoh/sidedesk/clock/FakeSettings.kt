package io.github.hyeonsikoh.sidedesk.clock

class FakeSettings(
    override var face: String? = null,
    override var theme: String? = null,
    override var askedLocation: Boolean = false,
    override var keepOn: String? = null,
    override var restAfter: String? = null,
    override var wakeOnAlert: Boolean = false,
    override var vibration: String? = null,
) : Settings
