package mycelium.state

import io.ktor.http.encodeURLParameter
import kotlin.time.Clock
import kotlin.time.Instant
import mycelium.Connection
import kotlinx.serialization.json.JsonPrimitive
import mycelium.api.BrowserTask
import mycelium.api.BrowserTaskState
import mycelium.api.Environment
import mycelium.api.EnvironmentCapability
import mycelium.api.EnvironmentApp
import mycelium.api.EnvironmentAppKind
import mycelium.api.EnvironmentController
import mycelium.api.EnvironmentState
import mycelium.api.EnvironmentVersions
import mycelium.api.EnvironmentWindow
import mycelium.api.NodeSummary

enum class EnvironmentStage { CannotRun, NoNode, Empty, Starting, Failed, Active, Stopped }

const val EnvironmentStoppedAdvice = "The browser closed with it, so anything signed in there is signed out."

fun environmentStage(environment: Environment?, capability: EnvironmentCapability?, noNode: Boolean = false): EnvironmentStage = when {
    environment?.state == EnvironmentState.Starting -> EnvironmentStage.Starting
    environment?.state == EnvironmentState.Running -> EnvironmentStage.Active
    environment?.state == EnvironmentState.Failed -> EnvironmentStage.Failed
    capability != null && !capability.available -> EnvironmentStage.CannotRun
    noNode && (environment == null || environment.state == EnvironmentState.Stopped) -> EnvironmentStage.NoNode
    environment?.state == EnvironmentState.Stopped -> EnvironmentStage.Stopped
    else -> EnvironmentStage.Empty
}

fun environmentStatus(controller: EnvironmentController): String = when (controller) {
    EnvironmentController.Agent -> "Agent in control"
    EnvironmentController.Person -> "You are in control"
}

fun environmentActivity(environment: Environment): String =
    if (environment.browserTask?.state == BrowserTaskState.Running) "Browser task" else environmentStatus(environment.controller)

fun browserTaskSeconds(millis: Long): String {
    val tenths = (millis + 50) / 100

    return "${tenths / 10}.${tenths % 10} s"
}

fun browserTaskStepsTotal(task: BrowserTask): String = browserTaskSeconds(task.steps.sumOf { it.millis })

fun browserTaskStopped(task: BrowserTask): String {
    val steps = "${task.steps.size} ${if (task.steps.size == 1) "step" else "steps"}"

    return if (task.stoppedBy == "took_over") "Stopped when you took over, after $steps." else "Stopped after $steps."
}

fun browserTaskGoal(call: ThreadItem.ToolCall): String? {
    if (!call.title.contains("browser_task")) {
        return null
    }

    return (call.input?.get("goal") as? JsonPrimitive)?.takeIf { it.isString }?.content
}

fun environmentControl(controller: EnvironmentController, person: String?): String = when (controller) {
    EnvironmentController.Agent -> "Agent in control"
    EnvironmentController.Person -> "${person ?: "A person"} in control"
}

fun environmentStoppedText(environment: Environment, localDateTime: (Long) -> String?): String {
    val moment = environment.stoppedAt?.let(::instantOrNull)
    val time = moment?.let { (localDateTime(it.toEpochMilliseconds()) ?: it.toString()).substringAfter('T').take(5) }

    if (environment.closedWithConversation) {
        return "It stopped when you closed this conversation${time?.let { " at $it" }.orEmpty()}, so the apps in it closed too."
    }

    return if (time == null) "You stopped it. $EnvironmentStoppedAdvice" else "You stopped it at $time. $EnvironmentStoppedAdvice"
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

fun environmentSizeText(environment: Environment): String = "${environment.size.width} × ${environment.size.height}"

fun environmentStartHint(nodeName: String): String = "Apps on $nodeName. You and the agent can open others once it is running."

fun environmentDefaultApps(apps: List<EnvironmentApp>): Set<String> = setOfNotNull(apps.firstOrNull { it.kind == EnvironmentAppKind.Browser }?.id)

fun environmentAppsToStart(apps: List<EnvironmentApp>, ticked: Set<String>): List<String> = apps.filter { it.id in ticked }.map { it.id }

fun environmentAppName(apps: List<EnvironmentApp>, appId: String): String =
    apps.firstOrNull { it.id.equals(appId, ignoreCase = true) }?.name ?: appId

fun environmentKindText(kind: EnvironmentAppKind): String = when (kind) {
    EnvironmentAppKind.Browser -> "Browser"
    EnvironmentAppKind.FileManager -> "File manager"
    EnvironmentAppKind.Editor -> "Editor"
    EnvironmentAppKind.Office -> "Office"
    EnvironmentAppKind.Mail -> "Mail"
    EnvironmentAppKind.Terminal -> "Terminal"
    EnvironmentAppKind.Other -> "Other"
}

fun environmentCurrentWindow(environment: Environment): EnvironmentWindow? =
    environment.windows.firstOrNull { it.focused } ?: environment.windows.firstOrNull()

fun environmentWindowApp(environment: Environment, window: EnvironmentWindow, apps: List<EnvironmentApp>): String {
    val name = environmentAppName(apps, window.appId)

    return if (window.id == environment.agentWindow) "$name · agent" else name
}

fun environmentOpenApps(environment: Environment, apps: List<EnvironmentApp>): String =
    environment.windows.map { environmentAppName(apps, it.appId) }.distinct().joinToString(", ")

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

const val EnvironmentNoNodeText = "No node that can run an environment is online. Start one from Nodes."
const val EnvironmentLastUsedHint = "Last used"

fun environmentNodes(nodes: Collection<NodeSummary>): List<NodeSummary> =
    nodes.filter { it.online && it.environment?.available == true }.sortedBy { it.name }

fun environmentRunOn(eligible: List<NodeSummary>, lastUsedId: String?, chosenId: String?): NodeSummary? =
    eligible.firstOrNull { it.id == chosenId } ?: eligible.firstOrNull { it.id == lastUsedId } ?: eligible.firstOrNull()

fun nodeThatCanRunEnvironment(nodes: Collection<NodeSummary>, currentNodeId: String): NodeSummary? =
    nodes.filter { it.id != currentNodeId && it.online && !it.draining && it.environment?.available == true }.minByOrNull { it.name }

fun environmentViewUrl(baseUrl: String, conversationId: String, ticket: String): String {
    val stream = "/api/v1/conversations/$conversationId/environment/stream"

    return "${baseUrl.trimEnd('/')}/novnc/embed.html?path=${stream.encodeURLParameter()}&ticket=${ticket.encodeURLParameter()}"
}
