package mycelium.state

import mycelium.CommandOutcome
import mycelium.Platform
import mycelium.api.ApiException
import mycelium.api.MyceliumApi
import mycelium.api.NodeSummary

suspend fun openInIde(api: MyceliumApi, platform: Platform, node: NodeSummary, ide: Ide, settings: IdeSettings, path: String, line: Int?) {
    val remote = !runsOnThisComputer(node.hostname, platform.hostname())
    val command = settings.commandFor(ide)
    var sshHost: String? = null

    if (remote) {
        val host = sshHostAlias(node.name)
        val failure = platform.prepareSshHost(host, node.id, api.mintSshToken(node.id).token)

        if (failure != null) {
            throw ApiException(0, "ide_ssh_unavailable", failure)
        }

        sshHost = host
    }

    if (platform.runCommand(listOf(command) + ideArguments(ide, path, line, sshHost)) == CommandOutcome.NotFound) {
        throw ApiException(0, "ide_not_found", ideNotFound(ide, command))
    }
}

class IdeLauncher(private val store: Store, private val platform: Platform, val openSettings: () -> Unit) {
    private fun settings(): IdeSettings = ideSettings(platform.loadPreference(IdePreference))

    private fun remote(node: NodeSummary): Boolean = !runsOnThisComputer(node.hostname, platform.hostname())

    fun main(node: NodeSummary): Ide? = settings().main(remote(node))

    fun others(node: NodeSummary): List<Ide> = settings().let { it.offered(remote(node)) - it.main(remote(node)) }.filterNotNull()

    fun where(node: NodeSummary): String = if (remote(node)) "On ${node.name}, over SSH" else "On this computer"

    fun open(node: NodeSummary, ide: Ide, path: String, line: Int? = null) {
        store.act { openInIde(store.api, platform, node, ide, settings(), path, line) }
    }
}
