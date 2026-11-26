package io.github.hyeonsikoh.sidedesk.link

import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow

class FakeLink : ComputerLink {
    override val state = MutableStateFlow<LinkState>(LinkState.Waiting)
    override val perf = MutableStateFlow<Perf?>(null)
    override val processes = MutableStateFlow<Processes?>(null)
    override val weather = MutableStateFlow<Weather?>(null)
    override val sessions = MutableStateFlow<List<AiSession>?>(null)
    override val forge = MutableStateFlow<Forge?>(null)
    override val alerts = MutableSharedFlow<AiAlert>(extraBufferCapacity = 8)
    val locations = mutableListOf<LocationState>()
    override val results = MutableSharedFlow<CommandResult>(extraBufferCapacity = 8)
    val answers = mutableListOf<Boolean>()
    val subscriptions = mutableListOf<Subscription>()
    val kills = mutableListOf<Command>()
    override val shortcuts = MutableStateFlow<List<Shortcut>?>(null)
    /** runShortcut 이 받아들일지 */
    var acceptShortcuts = true

    override fun start() = Unit

    override fun stop() = Unit

    override fun answer(same: Boolean) {
        answers += same
    }

    override fun subscribe(subscription: Subscription) {
        subscriptions += subscription
    }

    override fun setLocation(location: LocationState) {
        locations += location
    }

    override fun send(command: Command) {
        kills += command
    }

    override fun runShortcut(command: RunShortcut): Boolean {
        if (!acceptShortcuts) return false
        kills += command
        return true
    }

    val givenUp = mutableListOf<Long>()

    override fun giveUp(commandId: Long) {
        givenUp += commandId
    }
}
