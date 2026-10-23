package mycelium.ui

import kotlinx.serialization.Serializable
import kotlinx.serialization.builtins.MapSerializer
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.builtins.serializer
import kotlinx.serialization.json.Json
import mycelium.api.ActivityRow
import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.state.agentName

@Serializable
enum class HistoryColumn(val title: String, val numeric: Boolean = false, val standard: Boolean = true) {
    Title("Conversation"),
    Project("Project"),
    Node("Node"),
    Agent("Agent"),
    Model("Model", standard = false),
    Started("Started", standard = false),
    LastActive("Last active"),
    Turns("Turns", numeric = true),
    Tokens("Tokens", numeric = true),
    Cost("Cost", numeric = true),
    State("State", standard = false),
}

@Serializable
enum class SortDirection { Ascending, Descending }

@Serializable
data class HistorySort(val column: HistoryColumn, val direction: SortDirection)

@Serializable
enum class HistoryGrouping(val label: String, val choice: String) {
    None("Group", "No grouping"),
    Project("Grouped by project", "By project"),
    Node("Grouped by node", "By node"),
}

@Serializable
data class HistoryFilters(
    val project: String? = null,
    val node: String? = null,
    val agent: String? = null,
    val state: PastFilter = PastFilter.All,
)

@Serializable
data class HistoryLayout(
    val columns: List<HistoryColumn> = standardHistoryColumns(),
    val sort: HistorySort = HistorySort(HistoryColumn.LastActive, SortDirection.Descending),
    val filters: HistoryFilters = HistoryFilters(),
    val grouping: HistoryGrouping = HistoryGrouping.None,
)

@Serializable
data class HistoryView(val name: String, val layout: HistoryLayout)

data class HistoryEntry(val conversation: Conversation, val project: String, val node: String, val usage: ActivityRow?) {
    val agent: String get() = agentName(conversation.agent)

    val state: String get() = historyStateName(conversation)
}

data class HistoryGroup(val key: String, val label: String, val rows: List<HistoryEntry>)

data class HistoryOptions(val projects: List<String>, val nodes: List<String>, val agents: List<String>)

private val HistoryJson = Json {
    ignoreUnknownKeys = true
    coerceInputValues = true
    encodeDefaults = true
}

fun standardHistoryColumns(): List<HistoryColumn> = HistoryColumn.entries.filter { it.standard }

fun historyStateName(conversation: Conversation): String = when {
    conversation.archivedAt != null -> "Archived"
    conversation.status == ConversationStatus.Input -> "Needs input"
    conversation.status == ConversationStatus.Failed -> "Failed"
    conversation.status == ConversationStatus.Review -> "Ready to review"
    conversation.status == ConversationStatus.Working -> "Working"
    else -> "Done"
}

fun HistorySort.after(clicked: HistoryColumn): HistorySort {
    val ascending = HistorySort(clicked, SortDirection.Ascending)

    return if (column == clicked && direction == SortDirection.Ascending) HistorySort(clicked, SortDirection.Descending) else ascending
}

fun historyOptions(rows: List<HistoryEntry>): HistoryOptions = HistoryOptions(
    projects = rows.map { it.project }.filter { it.isNotEmpty() }.distinct().sortedBy { it.lowercase() },
    nodes = rows.map { it.node }.filter { it.isNotEmpty() }.distinct().sortedBy { it.lowercase() },
    agents = rows.map { it.agent }.filter { it.isNotEmpty() }.distinct().sortedBy { it.lowercase() },
)

fun filterHistory(rows: List<HistoryEntry>, filters: HistoryFilters): List<HistoryEntry> = rows.filter { row ->
    (filters.project == null || row.project == filters.project) &&
        (filters.node == null || row.node == filters.node) &&
        (filters.agent == null || row.agent == filters.agent)
}

private fun <T : Comparable<T>> nullsLast(direction: SortDirection, key: (HistoryEntry) -> T?): Comparator<HistoryEntry> = Comparator { left, right ->
    val first = key(left)
    val second = key(right)

    when {
        first == null && second == null -> 0
        first == null -> 1
        second == null -> -1
        direction == SortDirection.Ascending -> first.compareTo(second)
        else -> second.compareTo(first)
    }
}

fun sortHistory(rows: List<HistoryEntry>, sort: HistorySort): List<HistoryEntry> {
    val direction = sort.direction
    val comparator = when (sort.column) {
        HistoryColumn.Title -> nullsLast(direction) { it.conversation.title.lowercase() }
        HistoryColumn.Project -> nullsLast(direction) { it.project.lowercase().ifEmpty { null } }
        HistoryColumn.Node -> nullsLast(direction) { it.node.lowercase().ifEmpty { null } }
        HistoryColumn.Agent -> nullsLast(direction) { it.agent.lowercase() }
        HistoryColumn.Model -> nullsLast(direction) { it.conversation.model?.lowercase()?.ifEmpty { null } }
        HistoryColumn.Started -> nullsLast(direction) { it.conversation.createdAt }
        HistoryColumn.LastActive -> nullsLast(direction) { it.conversation.updatedAt }
        HistoryColumn.Turns -> nullsLast(direction) { it.usage?.turns ?: 0L }
        HistoryColumn.Tokens -> nullsLast(direction) { row -> row.usage?.takeIf { it.reportedTurns > 0 }?.tokens }
        HistoryColumn.Cost -> nullsLast(direction) { it.usage?.costUsd }
        HistoryColumn.State -> nullsLast(direction) { it.state }
    }

    return rows.sortedWith(comparator)
}

fun groupHistory(rows: List<HistoryEntry>, grouping: HistoryGrouping): List<HistoryGroup> {
    if (grouping == HistoryGrouping.None) {
        return listOf(HistoryGroup("", "", rows))
    }

    val keyOf = { row: HistoryEntry -> if (grouping == HistoryGrouping.Project) row.project else row.node }
    val empty = if (grouping == HistoryGrouping.Project) "No project" else "No node"

    return rows
        .groupBy(keyOf)
        .map { (key, members) -> HistoryGroup(key, key.ifEmpty { empty }, members) }
        .sortedWith(compareBy({ it.key.isEmpty() }, { it.key.lowercase() }))
}

fun historyViewNameProblem(views: List<HistoryView>, name: String, renaming: String? = null): String? {
    val trimmed = name.trim()

    return when {
        trimmed.isEmpty() -> "Enter a name for the view"
        views.any { it.name.equals(trimmed, ignoreCase = true) && !it.name.equals(renaming, ignoreCase = true) } -> "A view called $trimmed already exists. Enter a different name"
        else -> null
    }
}

fun saveHistoryView(views: List<HistoryView>, name: String, layout: HistoryLayout): List<HistoryView> {
    val saved = HistoryView(name.trim(), layout)

    return if (views.any { it.name.equals(saved.name, ignoreCase = true) }) {
        views.map { if (it.name.equals(saved.name, ignoreCase = true)) saved else it }
    } else {
        views + saved
    }
}

fun renameHistoryView(views: List<HistoryView>, name: String, newName: String): List<HistoryView> =
    views.map { if (it.name == name) it.copy(name = newName.trim()) else it }

fun deleteHistoryView(views: List<HistoryView>, name: String): List<HistoryView> = views.filter { it.name != name }

fun encodeHistoryViews(views: List<HistoryView>): String = HistoryJson.encodeToString(ListSerializer(HistoryView.serializer()), views)

fun decodeHistoryViews(stored: String?): List<HistoryView> =
    stored?.let { runCatching { HistoryJson.decodeFromString(ListSerializer(HistoryView.serializer()), it) }.getOrNull() }.orEmpty()

fun encodeHistoryColumns(columns: List<HistoryColumn>): String = HistoryJson.encodeToString(ListSerializer(HistoryColumn.serializer()), columns)

fun decodeHistoryColumns(stored: String?): List<HistoryColumn> =
    stored?.let { runCatching { HistoryJson.decodeFromString(ListSerializer(HistoryColumn.serializer()), it) }.getOrNull() }
        ?.takeIf { it.isNotEmpty() }
        ?: standardHistoryColumns()

fun encodeHistoryWidths(widths: Map<HistoryColumn, Float>): String = HistoryJson.encodeToString(MapSerializer(HistoryColumn.serializer(), Float.serializer()), widths)

fun decodeHistoryWidths(stored: String?): Map<HistoryColumn, Float> =
    stored?.let { runCatching { HistoryJson.decodeFromString(MapSerializer(HistoryColumn.serializer(), Float.serializer()), it) }.getOrNull() }.orEmpty()
