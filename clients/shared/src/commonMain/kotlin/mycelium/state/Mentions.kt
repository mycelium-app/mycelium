package mycelium.state

import mycelium.api.Assistant
import mycelium.api.ChatToolServer
import mycelium.api.MentionTrigger
import mycelium.api.Project
import mycelium.api.SaveMentionTriggerRequest

private val MentionKinds = setOf("stoat", "discord")

fun takesMentions(server: ChatToolServer): Boolean = server.kind in MentionKinds

fun mentionsHint(kind: String): String =
    if (kind == "discord") "When someone mentions the bot or messages it directly, the first rule that matches decides where it goes." else "When someone mentions you or messages you directly, the first rule that matches decides where it goes."

fun mentionWhen(kind: String): String = if (kind == "discord") "Someone mentions the bot on Discord" else "Someone mentions you on Stoat"

fun mentionPlatform(kind: String): String = if (kind == "discord") "Discord" else "Stoat"

fun mentionRules(rules: List<MentionTrigger>, integration: String): List<MentionTrigger> = rules.filter { it.integration == integration }.sortedBy { it.position }

fun mentionIn(rule: SaveMentionTriggerRequest, accounts: List<ChatToolServer>): String = when {
    rule.guildId != null -> rule.guildName ?: rule.guildId
    rule.serverId != null -> accounts.firstOrNull { it.id == rule.serverId }?.let { "Any server on ${it.account ?: it.name}" } ?: "Any server"
    else -> "Any server"
}

fun mentionFrom(rule: SaveMentionTriggerRequest): String = if (rule.senders.isEmpty()) "Everyone" else rule.senders.indices.joinToString(", ") { rule.senderNames.getOrNull(it)?.ifBlank { null } ?: rule.senders[it] }

fun mentionTarget(rule: SaveMentionTriggerRequest): String? = when {
    rule.assistantId != null -> "assistant:${rule.assistantId}"
    rule.projectId != null && rule.agent != null -> "agent:${rule.projectId}:${rule.agent}"
    else -> null
}

fun withMentionTarget(rule: SaveMentionTriggerRequest, target: String): SaveMentionTriggerRequest {
    val parts = target.split(":")

    return when (parts.first()) {
        "assistant" -> rule.copy(assistantId = parts[1], projectId = null, agent = null)
        else -> rule.copy(assistantId = null, projectId = parts[1], agent = parts[2])
    }
}

fun mentionTargetName(rule: SaveMentionTriggerRequest, assistants: Map<String, Assistant>, projects: Map<String, Project>): String? = when {
    rule.assistantId != null -> assistants[rule.assistantId]?.name ?: "An assistant"
    rule.projectId != null && rule.agent != null -> "Agent · ${agentName(rule.agent)} in ${projects[rule.projectId]?.name ?: "a project"}"
    else -> null
}

fun editable(rule: MentionTrigger): SaveMentionTriggerRequest =
    SaveMentionTriggerRequest(rule.integration, rule.serverId, rule.guildId, rule.guildName, rule.assistantId, rule.projectId, rule.agent, rule.channels, rule.senders, rule.senderNames)
