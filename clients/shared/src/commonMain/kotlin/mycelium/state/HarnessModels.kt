package mycelium.state

import mycelium.api.HarnessModels
import mycelium.api.NodeSummary
import kotlin.time.Clock
import kotlin.time.Instant

const val DefaultModelId = ""

data class ModelMenu(val title: String, val choices: List<Choice>)

private fun reportFor(node: NodeSummary, agent: String, account: String?): HarnessModels? =
    node.harnessModels.firstOrNull { it.agent == agent && it.account == account } ?: node.harnessModels.firstOrNull { it.agent == agent }

private fun defaultChoice(report: HarnessModels?): Choice =
    Choice(DefaultModelId, "Default model", report?.let { held -> held.models.firstOrNull { it.id == held.defaultModel }?.name ?: held.defaultModel })

private fun reportedAgo(iso: String, now: Instant): String {
    val minutes = (now - (instantOrNull(iso) ?: now)).inWholeMinutes

    return when {
        minutes < 1 -> "just now"
        minutes < 60 -> "$minutes ${if (minutes == 1L) "minute" else "minutes"} ago"
        minutes < 60 * 24 -> (minutes / 60).let { "$it ${if (it == 1L) "hour" else "hours"} ago" }
        else -> (minutes / (60 * 24)).let { "$it ${if (it == 1L) "day" else "days"} ago" }
    }
}

fun modelMenu(node: NodeSummary, agent: String, account: String?, now: Instant = Clock.System.now()): ModelMenu {
    val report = reportFor(node, agent, account)

    if (report == null || report.models.isEmpty()) {
        return ModelMenu("${node.name} has not reported its models yet. They appear after its first session.", listOf(defaultChoice(null)))
    }

    return ModelMenu("${agentName(agent)} on ${node.name}, ${reportedAgo(report.reportedAt, now)}", listOf(defaultChoice(report)) + report.models.map { Choice(it.id, it.name) })
}

fun fallbackModes(agent: String): List<Choice> = when (agent) {
    "claude" -> listOf(
        Choice("default", "Manual"),
        Choice("acceptEdits", "Accept edits"),
        Choice("plan", "Plan"),
        Choice("auto", "Auto"),
        Choice("bypassPermissions", "Bypass permissions"),
    )
    "codex" -> listOf(
        Choice("read-only", "Read-only"),
        Choice("workspace-write", "Workspace access"),
        Choice("agent", "Auto review"),
        Choice("agent-full-access", "Full access"),
    )
    else -> emptyList()
}

fun reportedModes(nodes: Collection<NodeSummary>, agent: String, account: String? = null): List<Choice> {
    val reports = nodes.mapNotNull { reportFor(it, agent, account) }.filter { it.modes.isNotEmpty() }.sortedByDescending { it.reportedAt }
    val modes = reports.flatMap { it.modes }.distinctBy { it.id }.map { Choice(it.id, it.name) }

    return modes.ifEmpty { fallbackModes(agent) }
}

fun reportedModelChoices(node: NodeSummary?, agent: String, account: String?): List<Choice> =
    node?.let { reportFor(it, agent, account) }?.models.orEmpty().map { Choice(it.id, it.name) }

fun reportedEffort(node: NodeSummary?, agent: String?, account: String?, model: String): Setting? {
    val report = node?.harnessModels?.firstOrNull { it.agent == agent && it.account == account } ?: return null

    if (model.isNotEmpty() && model != report.defaultModel) {
        return null
    }

    return settingsFrom(report.configOptions).firstOrNull {
        it.choices.isNotEmpty() && (it.category == "thought_level" || it.id.contains("effort") || it.id.contains("reasoning"))
    }
}
