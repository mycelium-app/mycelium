package mycelium.state

import mycelium.api.ChatToolInfo
import mycelium.api.IntegrationPolicy
import mycelium.api.PolicyLevel
import mycelium.api.PresetAskAll
import mycelium.api.PresetAskAllEdit
import mycelium.api.PresetAskWrites
import mycelium.api.PresetDontAsk

data class ApprovalPresetOption(val id: String, val name: String, val hint: String)

val ApprovalPresets = listOf(
    ApprovalPresetOption(PresetAskWrites, "Ask before writes", "Reads run straight away. Every write asks you first."),
    ApprovalPresetOption(PresetAskAll, "Ask before everything", "Every call asks you first."),
    ApprovalPresetOption(PresetAskAllEdit, "Ask before everything and edit responses", "Every call asks you first, and you can edit the response before the agent sees it."),
    ApprovalPresetOption(PresetDontAsk, "Don't ask", "Every call runs straight away."),
)

enum class ApprovalColumn { Ask, Edit }

enum class GroupTick { None, Some, All }

data class ResolvedApproval(val value: Boolean, val own: Boolean)

fun presetOption(id: String): ApprovalPresetOption = ApprovalPresets.firstOrNull { it.id == id } ?: ApprovalPresets.first()

private fun IntegrationPolicy.level(column: ApprovalColumn): PolicyLevel = if (column == ApprovalColumn.Ask) ask else edit

private fun IntegrationPolicy.withLevel(column: ApprovalColumn, level: PolicyLevel): IntegrationPolicy = if (column == ApprovalColumn.Ask) copy(ask = level) else copy(edit = level)

private fun PolicyLevel.byType(kind: String): Boolean? = if (kind == "read") read else write

private fun PolicyLevel.withType(kind: String, value: Boolean?): PolicyLevel = if (kind == "read") copy(read = value) else copy(write = value)

fun presetAnswer(preset: String, column: ApprovalColumn, kind: String): Boolean = when (column) {
    ApprovalColumn.Ask -> when (preset) {
        PresetAskWrites -> kind != "read"
        PresetDontAsk -> false
        else -> true
    }
    ApprovalColumn.Edit -> preset == PresetAskAllEdit
}

private fun followed(policy: IntegrationPolicy, column: ApprovalColumn, kind: String): Boolean = policy.level(column).byType(kind) ?: presetAnswer(policy.preset, column, kind)

fun resolveApproval(policy: IntegrationPolicy, column: ApprovalColumn, tool: ChatToolInfo): ResolvedApproval {
    val level = policy.level(column)
    val own = level.tools[tool.name]

    if (own != null) {
        return ResolvedApproval(own, true)
    }

    return ResolvedApproval(followed(policy, column, tool.kind), false)
}

fun groupTick(policy: IntegrationPolicy, column: ApprovalColumn, tools: List<ChatToolInfo>): GroupTick {
    val values = tools.map { resolveApproval(policy, column, it).value }

    return when {
        values.isNotEmpty() && values.all { it } -> GroupTick.All
        values.any { it } -> GroupTick.Some
        else -> GroupTick.None
    }
}

fun groupFollowsPreset(policy: IntegrationPolicy, column: ApprovalColumn, kind: String, tools: List<ChatToolInfo>): Boolean = policy.level(column).byType(kind) == null && tools.none { it.name in policy.level(column).tools }

fun toggleGroup(policy: IntegrationPolicy, column: ApprovalColumn, kind: String, tools: List<ChatToolInfo>): IntegrationPolicy {
    val wanted = groupTick(policy, column, tools) != GroupTick.All
    val level = policy.level(column)
    val rule = if (wanted == presetAnswer(policy.preset, column, kind)) null else wanted

    return policy.withLevel(column, level.withType(kind, rule).copy(tools = level.tools - tools.map { it.name }.toSet()))
}

fun toggleTool(policy: IntegrationPolicy, column: ApprovalColumn, tool: ChatToolInfo): IntegrationPolicy {
    val level = policy.level(column)
    val wanted = !resolveApproval(policy, column, tool).value
    val tools = if (wanted == followed(policy, column, tool.kind)) level.tools - tool.name else level.tools + (tool.name to wanted)

    return policy.withLevel(column, level.copy(tools = tools))
}

fun resetTool(policy: IntegrationPolicy, tool: ChatToolInfo): IntegrationPolicy = policy.copy(ask = policy.ask.copy(tools = policy.ask.tools - tool.name), edit = policy.edit.copy(tools = policy.edit.tools - tool.name))

fun toolIsCustomised(policy: IntegrationPolicy, tool: ChatToolInfo): Boolean = tool.name in policy.ask.tools || tool.name in policy.edit.tools

fun isCustomised(policy: IntegrationPolicy): Boolean = listOf(policy.ask, policy.edit).any { it.read != null || it.write != null || it.tools.isNotEmpty() }

fun changePreset(policy: IntegrationPolicy, preset: String): IntegrationPolicy = policy.copy(preset = preset)

fun presetSummary(policy: IntegrationPolicy): String = presetOption(policy.preset).name + if (isCustomised(policy)) ", customised" else ""

fun askWord(policy: IntegrationPolicy, tool: ChatToolInfo): String {
    val resolved = resolveApproval(policy, ApprovalColumn.Ask, tool)
    val presetAsks = presetAnswer(policy.preset, ApprovalColumn.Ask, tool.kind)

    return when {
        resolved.value -> "Yes"
        presetAsks -> "Always approved"
        else -> "No"
    }
}

fun editWord(policy: IntegrationPolicy, tool: ChatToolInfo): String = if (resolveApproval(policy, ApprovalColumn.Edit, tool).value) "Yes" else "No"
