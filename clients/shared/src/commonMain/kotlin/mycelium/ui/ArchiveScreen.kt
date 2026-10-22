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
import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.api.PatchConversationRequest
import mycelium.state.AppState
import mycelium.state.Store
import mycelium.state.agentName
import mycelium.state.relativeTime

enum class PastFilter(val label: String) { All("All"), Done("Done"), Archived("Archived") }

fun pastConversations(done: Collection<Conversation>, archived: Collection<Conversation>, filter: PastFilter, query: String): List<Conversation> {
    val doneRows = done.filter { it.status == ConversationStatus.Done && it.archivedAt == null }
    val source = when (filter) {
        PastFilter.All -> doneRows + archived
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

    val rows = pastConversations(state.conversations.values, archived, filter, query)

    Column(Modifier.fillMaxSize()) {
        Bar {
            BarTitle("Past conversations")
            Box(Modifier.weight(1f))
            TextInput(query, { query = it }, label = "Search past conversations", placeholder = "Search titles", showLabel = false, modifier = Modifier.width(Tokens.Size.FormColumn))
        }

        Bar {
            Segmented(PastFilter.entries.map { it.label }, filter.label, { picked -> filter = PastFilter.entries.first { it.label == picked } }, groupLabel = "State")
        }

        Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState())) {
            TableRow(header = true, "Conversation", "Agent", "Node", "Last active", "")

            if (rows.isEmpty()) {
                Label("No past conversations match.", Modifier.padding(Tokens.Space.S4), color = colors.textMuted)
            }

            for (row in rows) {
                val isArchived = row.archivedAt != null

                Row(
                    Modifier
                        .fillMaxWidth()
                        .heightIn(min = Tokens.Size.Row)
                        .padding(horizontal = Tokens.Space.S4),
                    horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Row(
                        Modifier
                            .weight(3f)
                            .pressable({ onOpen(row.id) }, role = Role.Button, description = "Open ${row.title}", shape = ControlShape),
                        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Label(row.title, Modifier.weight(1f, fill = false), maxLines = 1)

                        if (isArchived) {
                            Tag("Archived")
                        }
                    }

                    Label(agentName(row.agent), Modifier.weight(1.2f), color = colors.textMuted, maxLines = 1)
                    Label(state.nodes[row.nodeId]?.name.orEmpty(), Modifier.weight(1.2f), color = colors.textMuted, maxLines = 1)
                    Label(relativeTime(row.updatedAt), Modifier.weight(1f), color = colors.textMuted, maxLines = 1)

                    Box(Modifier.weight(1f), contentAlignment = Alignment.CenterEnd) {
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
            Modifier.fillMaxWidth().heightIn(min = Tokens.Size.Row).padding(horizontal = Tokens.Space.S4).semantics { contentDescription = "Columns" },
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
