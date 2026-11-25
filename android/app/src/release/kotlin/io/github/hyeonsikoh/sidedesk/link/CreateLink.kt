package io.github.hyeonsikoh.sidedesk.link

import kotlinx.coroutines.flow.MutableStateFlow

fun createLink(): ComputerLink = NoLink

private object NoLink : ComputerLink {
    override val state = MutableStateFlow<LinkState>(LinkState.Waiting)

    override fun start() = Unit

    override fun stop() = Unit
}
