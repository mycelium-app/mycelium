package mycelium.state

import mycelium.api.Assistant
import mycelium.api.Group

const val AllStates = "all"
const val GroupByGroup = "group"
const val GroupByState = "state"
const val GroupByNone = "none"
const val SidebarAssistantLimit = 5

data class AssistantSection(val title: String?, val rows: List<Assistant>)

private fun mostRecent(assistant: Assistant): String = assistant.lastActiveAt ?: assistant.createdAt

fun sidebarAssistants(assistants: Collection<Assistant>, groupFilter: String): List<Assistant> =
    assistants
        .filter { it.archivedAt == null && inGroup(it.groupId, groupFilter) && (it.pinnedAt != null || it.state != "idle") }
        .sortedWith(compareByDescending<Assistant> { it.pinnedAt != null }.thenByDescending(::mostRecent))
        .take(SidebarAssistantLimit)

fun assistantStateName(state: String): String = when (state) {
    "working" -> "Working"
    "input" -> "Needs input"
    else -> "Idle"
}

fun assistantRows(assistants: Collection<Assistant>, groups: Map<String, Group>, query: String, group: String, state: String, groupBy: String): List<AssistantSection> {
    val needle = query.trim().lowercase()
    val shown = assistants
        .filter { it.archivedAt == null && inGroup(it.groupId, group) && (state == AllStates || it.state == state) }
        .filter { needle.isEmpty() || needle in it.name.lowercase() || needle in it.description.orEmpty().lowercase() }
        .sortedWith(compareByDescending<Assistant> { it.pinnedAt != null }.thenBy { it.name.lowercase() })

    return when (groupBy) {
        GroupByGroup -> shown.groupBy { it.groupId }.map { (id, rows) -> AssistantSection(groups[id]?.name ?: "No group", rows) }.sortedBy { it.title }
        GroupByState -> shown.groupBy { it.state }.map { (name, rows) -> AssistantSection(assistantStateName(name), rows) }
        else -> if (shown.isEmpty()) emptyList() else listOf(AssistantSection(null, shown))
    }
}
