package io.github.hyeonsikoh.sidedesk.link

import kotlinx.coroutines.flow.StateFlow

sealed interface LinkState {
    data object Waiting : LinkState

    data class Received(val computerName: String) : LinkState

    data class Failed(val reason: String) : LinkState
}

interface ComputerLink {
    val state: StateFlow<LinkState>

    fun start()

    fun stop()
}
