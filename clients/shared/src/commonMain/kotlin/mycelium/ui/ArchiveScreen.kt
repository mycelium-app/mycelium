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
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
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
import androidx.compose.foundation.text.BasicText
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import mycelium.api.ApiException
import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.api.PatchConversationRequest
import mycelium.api.SearchHit
import mycelium.state.AppState
import mycelium.state.Store
import mycelium.state.agentName
import mycelium.state.relativeTime

private const val SearchDebounceMillis = 250L

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

    LaunchedEffect(Unit) {
        store.act { archived = store.api.conversations(archived = true) }
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
            Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState())) {
                if (found.isEmpty()) {
                    Label("No conversations match that search.", Modifier.padding(Tokens.Space.S4), color = colors.textMuted)
                }

                for (hit in found) {
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

        Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState())) {
            if (!touch) {
                TableRow(header = true, "Conversation", "Agent", "Node", "Last active", "")
            }

            if (rows.isEmpty()) {
                Label("No past conversations match.", Modifier.padding(Tokens.Space.S4), color = colors.textMuted)
            }

            for (row in rows) {
                val isArchived = row.archivedAt != null

                Row(
                    Modifier
                        .fillMaxWidth()
                        .heightIn(min = rowHeight())
                        .padding(horizontal = Tokens.Space.S2),
                    horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Column(
                        Modifier
                            .weight(if (touch) 1f else 3f)
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
                                listOf(agentName(row.agent), state.nodes[row.nodeId]?.name.orEmpty(), relativeTime(row.updatedAt)).filter { it.isNotEmpty() }.joinToString("  ·  "),
                                style = Tokens.Type.Meta,
                                color = colors.textMuted,
                                maxLines = 1,
                            )
                        }
                    }

                    if (!touch) {
                        Label(agentName(row.agent), Modifier.weight(1.2f), color = colors.textMuted, maxLines = 1)
                        Label(state.nodes[row.nodeId]?.name.orEmpty(), Modifier.weight(1.2f), color = colors.textMuted, maxLines = 1)
                        Label(relativeTime(row.updatedAt), Modifier.weight(1f), color = colors.textMuted, maxLines = 1)
                    }

                    Box(if (touch) Modifier else Modifier.weight(1f), contentAlignment = Alignment.CenterEnd) {
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

@Composable
private fun TableRow(header: Boolean, vararg cells: String) {
    val colors = MyTheme.colors
    val weights = listOf(3f, 1.2f, 1.2f, 1f, 1f)

    Column(Modifier.fillMaxWidth()) {
        Row(
            Modifier.fillMaxWidth().heightIn(min = rowHeight()).padding(horizontal = Tokens.Space.S4).semantics { contentDescription = "Columns" },
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            cells.forEachIndexed { index, cell ->
                Label(cell, Modifier.weight(weights[index]), if (header) Tokens.Type.MetaStrong else Tokens.Type.Ui, colors.textMuted, maxLines = 1)
            }
        }

        Hairline()
    }
}
