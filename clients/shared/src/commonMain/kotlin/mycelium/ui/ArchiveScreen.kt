package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import mycelium.state.AllGroups
import mycelium.state.inGroupFilter
import mycelium.state.projectName
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.foundation.text.BasicText
import kotlinx.coroutines.delay
import kotlinx.serialization.Serializable
import mycelium.api.ActivityRow
import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.api.PatchConversationRequest
import mycelium.api.SearchHit
import mycelium.state.AppState
import mycelium.state.Store
import mycelium.state.formatCost
import mycelium.state.formatTokens
import mycelium.state.isSubagent

private const val SearchDebounceMillis = 250L

private const val HistoryColumnsPreference = "history-columns"

private const val HistoryWidthsPreference = "history-widths"

private const val HistoryViewsPreference = "history-views"

fun usageCells(usage: ActivityRow?): List<String> {
    val reported = usage != null && usage.reportedTurns > 0

    return listOf(
        (usage?.turns ?: 0).toString(),
        when {
            !reported -> "Not reported"
            usage!!.subagentTokens > 0 -> "${formatTokens(usage.tokens)} (${formatTokens(usage.subagentTokens)} in subagents)"
            else -> formatTokens(usage.tokens)
        },
        if (usage?.costUsd == null) "Not reported" else formatCost(usage.costUsd) + if (usage.costEstimated) " (estimated)" else "",
    )
}

fun hasTurns(conversation: Conversation, usage: Map<String, ActivityRow>?): Boolean =
    usage == null || conversation.status == ConversationStatus.Working || (usage[conversation.id]?.turns ?: 0) > 0

@Serializable
enum class PastFilter(val label: String) { All("All"), Done("Done"), Archived("Archived") }

fun pastConversations(current: Collection<Conversation>, archived: Collection<Conversation>, filter: PastFilter, query: String, group: String = AllGroups): List<Conversation> {
    val doneRows = current.filter { it.status == ConversationStatus.Done && it.archivedAt == null }
    val source = when (filter) {
        PastFilter.All -> current.filter { it.archivedAt == null } + archived
        PastFilter.Done -> doneRows
        PastFilter.Archived -> archived.toList()
    }

    return source
        .distinctBy { it.id }
        .filter { !it.isSubagent && inGroupFilter(it, group) }
        .filter { query.isBlank() || it.title.contains(query.trim(), ignoreCase = true) }
        .sortedByDescending { it.updatedAt }
}

@Composable
fun ArchiveScreen(store: Store, state: AppState, onOpen: (String) -> Unit) {
    val colors = MyTheme.colors
    var archived by remember { mutableStateOf<List<Conversation>>(emptyList()) }
    var filter by remember { mutableStateOf(PastFilter.All) }
    var query by remember { mutableStateOf("") }
    var usage by remember { mutableStateOf<Map<String, ActivityRow>?>(null) }
    var deleting by remember { mutableStateOf<Conversation?>(null) }
    val platform = LocalPlatform.current
    var layout by remember { mutableStateOf(HistoryLayout(columns = decodeHistoryColumns(platform?.loadPreference(HistoryColumnsPreference)))) }
    var widths by remember { mutableStateOf(decodeHistoryWidths(platform?.loadPreference(HistoryWidthsPreference))) }
    var views by remember { mutableStateOf(decodeHistoryViews(platform?.loadPreference(HistoryViewsPreference))) }
    var activeView by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(layout.columns) {
        platform?.savePreference(HistoryColumnsPreference, encodeHistoryColumns(layout.columns))
    }

    LaunchedEffect(widths) {
        delay(SearchDebounceMillis)
        platform?.savePreference(HistoryWidthsPreference, encodeHistoryWidths(widths))
    }

    LaunchedEffect(Unit) {
        store.act { archived = store.api.conversations(archived = true) }
        store.act { usage = store.api.conversationUsage().associateBy { it.key } }
    }

    var hits by remember { mutableStateOf<List<SearchHit>?>(null) }
    var searchProblem by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(query) {
        if (query.isBlank()) {
            hits = null
            searchProblem = null

            return@LaunchedEffect
        }

        delay(SearchDebounceMillis)

        searchProblem = attempt { hits = store.api.search(query.trim()) }
    }

    val filters = layout.filters
    val entries = pastConversations(state.conversations.values, archived, filters.state, query, LocalGroupFilter.current)
        .filter { hasTurns(it, usage) }
        .map { HistoryEntry(it, projectName(it.cwd), state.nodes[it.nodeId]?.name.orEmpty(), usage?.get(it.id)) }
    val groups = groupHistory(sortHistory(filterHistory(entries, filters), layout.sort), layout.grouping)

    val touch = LocalTouch.current
    val stamp = timestamper()

    deleting?.let { doomed ->
        DeleteConversationDialog(
            doomed.title,
            onDismiss = { deleting = null },
            onConfirm = {
                deleting = null
                store.act {
                    store.api.deleteConversation(doomed.id)
                    archived = archived.filter { it.id != doomed.id }
                }
            },
        )
    }

    Column(Modifier.fillMaxSize()) {
        Bar {
            BarTitle("Past conversations")

            if (!touch) {
                Box(Modifier.weight(1f))
            }

            TextInput(
                query,
                { query = it },
                label = "Search past conversations",
                placeholder = "Search conversations",
                showLabel = false,
                modifier = if (touch) Modifier.weight(1f) else Modifier.width(Tokens.Size.FormColumn),
            )
        }

        if (query.isBlank()) {
            HistoryToolbar(
                layout,
                { layout = it },
                historyOptions(entries),
                views,
                { views = it; platform?.savePreference(HistoryViewsPreference, encodeHistoryViews(it)) },
                activeView,
                { activeView = it },
            )
        }

        val found = hits

        if (query.isNotBlank() && found != null) {
            val list = rememberLazyListState()

            LazyColumn(Modifier.weight(1f).fillMaxWidth().scrollbar(list), list) {
                if (found.isEmpty()) {
                    item(key = "empty") {
                        Label("No conversations match that search.", Modifier.padding(Tokens.Space.S4), color = colors.textMuted)
                    }
                }

                items(found, key = { it.conversationId }) { hit ->
                    Column(
                        Modifier
                            .fillMaxWidth()
                            .pressable({ onOpen(hit.conversationId) }, role = Role.Button, description = "Open ${hit.title}", shape = ControlShape)
                            .padding(horizontal = Tokens.Space.S4, vertical = Tokens.Space.S2),
                        verticalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5),
                    ) {
                        Label(hit.title, maxLines = 1)
                        Label(
                            listOf(state.nodes[hit.nodeId]?.name.orEmpty(), stamp(hit.updatedAt, false)).filter { it.isNotEmpty() }.joinToString("  ·  "),
                            style = Tokens.Type.Meta,
                            color = colors.textMuted,
                            maxLines = 1,
                        )
                        BasicText(inlineMarkup(hit.snippet, colors.textMuted, colors.accent), style = Tokens.Type.Meta.themed(), maxLines = 3)
                    }

                    Hairline()
                }
            }

            return@Column
        }

        if (query.isNotBlank() && searchProblem != null) {
            Label(searchProblem.orEmpty(), Modifier.padding(horizontal = Tokens.Space.S4, vertical = Tokens.Space.S2), color = colors.textMuted)
        }

        HistoryTable(groups, layout, { layout = it }, widths, { widths = it }, stamp, onOpen, Modifier.weight(1f)) { row ->
            val isArchived = row.archivedAt != null

            RowMenu(row.title) { close ->
                MenuItem(
                    if (isArchived) "Restore" else "Archive",
                    onClick = {
                        close()
                        store.act {
                            val updated = store.api.patchConversation(row.id, PatchConversationRequest(archived = !isArchived))
                            archived = if (isArchived) archived.filter { it.id != row.id } else archived + updated
                            store.refresh()
                        }
                    },
                    leading = MyIcon.Archive,
                )

                if (isArchived) {
                    MenuItem("Delete", onClick = { close(); deleting = row })
                }
            }
        }
    }
}
