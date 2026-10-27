package mycelium.state

import kotlinx.serialization.Serializable
import kotlinx.serialization.encodeToString
import mycelium.api.MyceliumJson

const val IdePreference = "ides"

enum class Ide(val id: String, val label: String, val defaultCommand: String, val reachesRemoteNodes: Boolean) {
    Zed("zed", "Zed", "zed", true),
    VsCode("code", "VS Code", "code", true),
    JetBrains("jetbrains", "JetBrains", "idea", false),
}

@Serializable
data class IdeChoice(val id: String, val offered: Boolean = true, val command: String = "")

@Serializable
data class IdeSettings(val choices: List<IdeChoice> = Ide.entries.map { IdeChoice(it.id) }, val default: String = Ide.Zed.id) {
    fun choiceFor(ide: Ide): IdeChoice = choices.firstOrNull { it.id == ide.id } ?: IdeChoice(ide.id)

    fun commandFor(ide: Ide): String = choiceFor(ide).command.trim().ifEmpty { ide.defaultCommand }

    fun offered(remote: Boolean): List<Ide> = Ide.entries.filter { choiceFor(it).offered && (!remote || it.reachesRemoteNodes) }

    fun main(remote: Boolean): Ide? = offered(remote).let { offered -> offered.firstOrNull { it.id == default } ?: offered.firstOrNull() }
}

fun ideSettings(stored: String?): IdeSettings =
    stored?.let { runCatching { MyceliumJson.decodeFromString<IdeSettings>(it) }.getOrNull() } ?: IdeSettings()

fun IdeSettings.stored(): String = MyceliumJson.encodeToString(this)

fun sshHostAlias(nodeName: String): String = "mycelium-" + nodeName.lowercase().replace(Regex("[^a-z0-9]+"), "-").trim('-')

fun ideArguments(ide: Ide, path: String, line: Int?, sshHost: String?): List<String> {
    val located = if (line != null) "$path:$line" else path

    return when {
        ide == Ide.Zed && sshHost != null -> listOf("ssh://$sshHost$located")
        ide == Ide.Zed -> listOf(located)
        ide == Ide.VsCode && sshHost != null -> listOf("--remote", "ssh-remote+$sshHost", "-g", located)
        ide == Ide.VsCode -> listOf("-g", located)
        line != null -> listOf("--line", line.toString(), path)
        else -> listOf(path)
    }
}

fun ideNotFound(ide: Ide, command: String): String = "${ide.label} was not found at $command. Set its command in Settings, IDEs."

fun runsOnThisComputer(nodeHostname: String, thisHostname: String?): Boolean =
    thisHostname != null && nodeHostname.substringBefore('.').equals(thisHostname.substringBefore('.'), ignoreCase = true)

fun absoluteInFolder(folder: String, path: String): String = if (path.startsWith("/")) path else folder.trimEnd('/') + "/" + path
