package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import mycelium.state.projectName
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.dp
import androidx.compose.foundation.text.BasicText
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import mycelium.api.ActivityRow
import mycelium.api.ApiException
import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.api.PatchConversationRequest
import mycelium.api.SearchHit
import mycelium.state.AppState
import mycelium.state.Store
import mycelium.state.agentName
import mycelium.state.formatCost
import mycelium.state.formatTokens
import mycelium.state.relativeTime

private const val SearchDebounceMillis = 250L

private val ActionWidth = 176.dp

private val PastHeaders = listOf("Conversation", "Project", "Agent", "Node", "Last active", "Turns", "Tokens", "Cost", "")

private val PastColumns = listOf(ColumnSpec(flexible = true), ColumnSpec(), ColumnSpec(), ColumnSpec(), ColumnSpec(), ColumnSpec(), ColumnSpec(), ColumnSpec(), ColumnSpec(min = ActionWidth, max = ActionWidth))

private val TouchColumns = listOf(ColumnSpec(flexible = true), ColumnSpec(min = ActionWidth, max = ActionWidth))

fun usageCells(usage: ActivityRow?): List<String> {
    val reported = usage != null && usage.reportedTurns > 0

    return listOf(
        (usage?.turns ?: 0).toString(),
        if (reported) formatTokens(usage!!.tokens) else "Not reported",
        if (usage?.costUsd == null) "Not reported" else formatCost(usage.costUsd) + if (usage.costEstimated) " (estimated)" else "",
    )
}

enum class PastFilter(val label: String) { All("All"), Done("Done"), Archived("Archived") }

fun pastConversations(current: Collection<Conversation>, archived: Collection<Conversation>, filter: PastFilter, query: String): List<Conversation> {
    val doneRows = current.filter { it.status == ConversationStatus.Done && it.archivedAt == null }
    val source = when (filter) {
        PastFilter.All -> current.filter { it.archivedAt == null } + archived
        PastFilter.Done -> doneRows
        PastFilter.Archived -> archived.toList()
    }

    return source
        .distinctBy { it.id }
        .filter { query.isBlank() || it.title.contains(query.trim(), ignoreCase = true) }
        .sortedByDescending { it.updatedAt }
}

@Composable
fun ArchiveScreen(store: Store, state: AppState, onOpen: (String) -> Unit) {
    val colors = MyTheme.colors
    var archived by remember { mutableStateOf<List<Conversation>>(emptyList()) }
    var filter by remember { mutableStateOf(PastFilter.All) }
    var query by remember { mutableStateOf("") }
    var usage by remember { mutableStateOf<Map<String, ActivityRow>>(emptyMap()) }
    var deleting by remember { mutableStateOf<Conversation?>(null) }

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

        try {
            hits = store.api.search(query.trim())
            searchProblem = null
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (failure: ApiException) {
            searchProblem = failure.message
        } catch (_: Exception) {
            searchProblem = "Could not reach the control plane. Check your connection and try again."
        }
    }

    val rows = pastConversations(state.conversations.values, archived, filter, query)

    val touch = LocalTouch.current
    val measurer = rememberTextMeasurer()
    val density = LocalDensity.current
    val bodyStyle = Tokens.Type.Ui.themed()
    val headerStyle = Tokens.Type.MetaStrong.themed()
    val pastCells = { row: Conversation -> listOf(projectName(row.cwd), agentName(row.agent), state.nodes[row.nodeId]?.name.orEmpty(), relativeTime(row.updatedAt)) + usageCells(usage[row.id]) }
    val naturalWidths = remember(rows, state.nodes, usage, touch, bodyStyle, headerStyle) {
        if (touch) {
            listOf(0.dp, ActionWidth)
        } else {
            val widest = { texts: List<String>, style: TextStyle -> with(density) { (texts.maxOfOrNull { measurer.measure(it, style).size.width } ?: 0).toDp() } }
            val cells = rows.map(pastCells)

            listOf(0.dp) + PastHeaders.drop(1).dropLast(1).mapIndexed { column, header ->
                widest(listOf(header), headerStyle).coerceAtLeast(widest(cells.map { it[column] }, bodyStyle))
            } + ActionWidth
        }
    }

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
            Bar {
                Segmented(PastFilter.entries.map { it.label }, filter.label, { picked -> filter = PastFilter.entries.first { it.label == picked } }, groupLabel = "State")
            }
        }

        val found = hits

        if (query.isNotBlank() && found != null) {
            LazyColumn(Modifier.weight(1f).fillMaxWidth()) {
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
                            listOf(state.nodes[hit.nodeId]?.name.orEmpty(), relativeTime(hit.updatedAt)).filter { it.isNotEmpty() }.joinToString("  ·  "),
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

        FixedWidthColumns(if (touch) TouchColumns else PastColumns, naturalWidths, Modifier.weight(1f)) {
            LazyColumn(Modifier.fillMaxWidth()) {
                if (!touch) {
                    item(key = "columns") {
                        ContentRow(Modifier.fillMaxWidth().semantics { contentDescription = "Columns" }) {
                            for (title in PastHeaders) {
                                Label(title, style = Tokens.Type.MetaStrong, color = colors.textMuted, maxLines = 1)
                            }
                        }

                        Hairline()
                    }
                }

                if (rows.isEmpty()) {
                    item(key = "empty") {
                        Label("No past conversations match.", Modifier.padding(Tokens.Space.S4), color = colors.textMuted)
                    }
                }

                items(rows, key = { it.id }) { row ->
                    val isArchived = row.archivedAt != null

                    ContentRow(Modifier.fillMaxWidth().hoverHighlight()) {
                        Column(
                            Modifier
                                .pressable({ onOpen(row.id) }, role = Role.Button, description = "Open ${row.title}", shape = ControlShape)
                                .padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1),
                            verticalArrangement = Arrangement.Center,
                        ) {
                            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                                Label(row.title, Modifier.weight(1f, fill = false), maxLines = 1)

                                if (isArchived) {
                                    Tag("Archived")
                                }
                            }

                            if (touch) {
                                Label(
                                    (listOf(projectName(row.cwd), agentName(row.agent), state.nodes[row.nodeId]?.name.orEmpty(), relativeTime(row.updatedAt)) + usageCells(usage[row.id]).drop(1)).filter { it.isNotEmpty() && it != "Not reported" }.joinToString("  ·  "),
                                    style = Tokens.Type.Meta,
                                    color = colors.textMuted,
                                    maxLines = 1,
                                )
                            }
                        }

                        if (!touch) {
                            for (cell in pastCells(row)) {
                                Label(cell, color = colors.textMuted, maxLines = 1)
                            }
                        }

                        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1, Alignment.End), verticalAlignment = Alignment.CenterVertically) {
                            if (isArchived) {
                                MyButton({ deleting = row }, text = "Delete", size = ButtonSize.Sm, muted = true, description = "Delete ${row.title}")
                            }

                            MyButton(
                                onClick = {
                                    store.act {
                                        val updated = store.api.patchConversation(row.id, PatchConversationRequest(archived = !isArchived))
                                        archived = if (isArchived) archived.filter { it.id != row.id } else archived + updated
                                        store.refresh()
                                    }
                                },
                                text = if (isArchived) "Restore" else "Archive",
                                size = ButtonSize.Sm,
                                muted = true,
                                description = "${if (isArchived) "Restore" else "Archive"} ${row.title}",
                            )
                        }
                    }

                    Hairline()
                }
            }
        }
    }
}
