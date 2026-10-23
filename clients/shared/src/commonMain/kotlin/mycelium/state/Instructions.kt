package mycelium.state

import mycelium.api.InstructionLayer
import mycelium.api.NodeSummary

fun shortVersion(id: String): String = id.take(7)

fun layerFile(layer: InstructionLayer, repository: String?): String =
    if (repository != null && layer.path != null) "${repository.substringAfterLast('/')} / ${layer.path}" else "Stored in Mycelium"

fun heldVersion(node: NodeSummary): String = node.instructionsRevision?.let(::shortVersion) ?: "Nothing yet"

fun olderSessions(node: NodeSummary): String = when {
    !node.online -> "Offline, catches up on reconnect"
    node.sessionsOnOlderInstructions == 0 -> "None"
    node.sessionsOnOlderInstructions == 1 -> "1 session"
    else -> "${node.sessionsOnOlderInstructions} sessions"
}
