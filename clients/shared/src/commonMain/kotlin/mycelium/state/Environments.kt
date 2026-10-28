package mycelium.state

import io.ktor.http.encodeURLParameter
import kotlin.time.Clock
import kotlin.time.Instant
import mycelium.Connection
import mycelium.api.Environment
import mycelium.api.EnvironmentCapability
import mycelium.api.EnvironmentController
import mycelium.api.EnvironmentState
import mycelium.api.EnvironmentVersions
import mycelium.api.NodeSummary

enum class EnvironmentStage { CannotRun, Empty, Starting, Failed, Active, Stopped }

const val EnvironmentStoppedAdvice = "The browser closed with it, so anything signed in there is signed out."

fun environmentStage(environment: Environment?, capability: EnvironmentCapability?): EnvironmentStage = when {
    environment?.state == EnvironmentState.Starting -> EnvironmentStage.Starting
    environment?.state == EnvironmentState.Running -> EnvironmentStage.Active
    environment?.state == EnvironmentState.Failed -> EnvironmentStage.Failed
    capability != null && !capability.available -> EnvironmentStage.CannotRun
    environment?.state == EnvironmentState.Stopped -> EnvironmentStage.Stopped
    else -> EnvironmentStage.Empty
}

fun environmentStatus(controller: EnvironmentController): String = when (controller) {
    EnvironmentController.Agent -> "Agent in control"
    EnvironmentController.Person -> "You are in control"
}

fun environmentControl(controller: EnvironmentController, person: String?): String = when (controller) {
    EnvironmentController.Agent -> "Agent in control"
    EnvironmentController.Person -> "${person ?: "A person"} in control"
}

fun environmentStoppedText(environment: Environment, localDateTime: (Long) -> String?): String {
    val moment = environment.stoppedAt?.let(::instantOrNull) ?: return "You stopped it. $EnvironmentStoppedAdvice"
    val local = localDateTime(moment.toEpochMilliseconds()) ?: moment.toString()

    return "You stopped it at ${local.substringAfter('T').take(5)}. $EnvironmentStoppedAdvice"
}

fun environmentTools(missing: List<String>): String = when (missing.size) {
    0, 1 -> missing.joinToString("")
    else -> missing.dropLast(1).joinToString(", ") + " and " + missing.last()
}

private fun toolsAreNotInstalled(missing: List<String>): String = "${environmentTools(missing)} ${if (missing.size == 1) "is" else "are"} not installed"

fun environmentMissingText(missing: List<String>, node: String): String =
    if (missing.isEmpty()) "Something an environment needs is missing on $node" else "${toolsAreNotInstalled(missing)} on $node"

fun environmentCannotRunText(capability: EnvironmentCapability): String =
    if (capability.missing.isEmpty()) capability.note ?: "Cannot run environments." else "Cannot run environments: ${toolsAreNotInstalled(capability.missing)}."

fun environmentVersionsText(versions: EnvironmentVersions): String = listOfNotNull(
    versions.compositor?.let { named("sway", it) },
    versions.wayvnc?.let { named("wayvnc", it) },
    versions.browser?.let { named("Chromium", it) },
).joinToString(", ")

private fun named(name: String, version: String): String = if (version.firstOrNull()?.isLetter() == true) version else "$name $version"

fun environmentSummary(environment: Environment): String =
    "Wayland session  ·  ${environment.size.width} × ${environment.size.height}  ·  ${environment.browser?.substringBefore(' ')?.takeIf { it.isNotBlank() } ?: "Chromium"}"

fun environmentDescription(environment: Environment): String {
    val doing = when (environment.controller) {
        EnvironmentController.Person -> "You are in control."
        EnvironmentController.Agent -> environment.target?.let { "The agent's pointer is at ${it.label}." } ?: "The agent is in control."
    }

    return "Live view of the environment on ${environment.nodeName}. $doing"
}

fun environmentActionAge(iso: String, now: Instant = Clock.System.now()): String {
    val seconds = instantOrNull(iso)?.let { (now - it).inWholeSeconds } ?: return ""

    return when {
        seconds < 5 -> "now"
        seconds < 60 -> "${seconds}s"
        else -> relativeTime(iso, now)
    }
}

fun environmentRunningFor(iso: String, now: Instant = Clock.System.now()): String {
    val minutes = instantOrNull(iso)?.let { (now - it).inWholeMinutes } ?: return ""

    return when {
        minutes < 1 -> "under 1 min"
        minutes < 60 -> "$minutes min"
        minutes < 24 * 60 -> "${minutes / 60} h"
        else -> "${minutes / (24 * 60)} d"
    }
}

fun nodeThatCanRunEnvironment(nodes: Collection<NodeSummary>, currentNodeId: String): NodeSummary? =
    nodes.filter { it.id != currentNodeId && it.online && !it.draining && it.environment?.available == true }.minByOrNull { it.name }

fun environmentViewUrl(baseUrl: String, conversationId: String, ticket: String): String {
    val stream = "/api/v1/conversations/$conversationId/environment/stream"

    return "${baseUrl.trimEnd('/')}/novnc/embed.html?path=${stream.encodeURLParameter()}&ticket=${ticket.encodeURLParameter()}"
}
