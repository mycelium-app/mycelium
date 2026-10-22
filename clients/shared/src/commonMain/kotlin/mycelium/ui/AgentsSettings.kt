package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import mycelium.api.ImportRequest
import mycelium.api.ImportStatus
import mycelium.state.Store
import mycelium.state.agentName

private val ImportableAgents = listOf("claude", "codex")

fun pendingImports(status: ImportStatus?): Int = ((status?.found ?: 0) - (status?.imported ?: 0)).coerceAtLeast(0)

@Composable
fun AgentsSettings(store: Store) {
    val colors = MyTheme.colors
    val state by store.state.collectAsState()
    var statuses by remember { mutableStateOf<List<ImportStatus>>(emptyList()) }
    var skipped by remember { mutableStateOf<Map<Pair<String, String>, Int>>(emptyMap()) }
    var importing by remember { mutableStateOf<Set<Pair<String, String>>>(emptySet()) }

    LaunchedEffect(Unit) {
        store.act { statuses = store.api.imports() }
    }

    Column(Modifier.widthIn(max = Tokens.Size.Page), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S6)) {
        for (agent in ImportableAgents) {
            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                Label(agentName(agent), Modifier.semantics { heading() }, Tokens.Type.UiStrong)

                Row(Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S2), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
                    Label("Node", Modifier.weight(2f), Tokens.Type.MetaStrong, colors.textMuted)
                    Label("Transcripts found", Modifier.weight(1.5f), Tokens.Type.MetaStrong, colors.textMuted)
                    Label("Imported", Modifier.weight(1f), Tokens.Type.MetaStrong, colors.textMuted)
                    Box(Modifier.weight(1.5f))
                }

                Hairline()

                for (node in state.nodes.values.sortedBy { it.name }) {
                    val status = statuses.firstOrNull { it.nodeId == node.id && it.agent == agent }
                    val pending = pendingImports(status)
                    val unreadable = skipped[node.id to agent] ?: 0

                    Row(
                        Modifier.fillMaxWidth().heightIn(min = rowHeight()).padding(horizontal = Tokens.Space.S2),
                        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Label(node.name, Modifier.weight(2f), maxLines = 1)
                        Label((status?.found ?: 0).toString(), Modifier.weight(1.5f), color = colors.textMuted)
                        Label((status?.imported ?: 0).toString(), Modifier.weight(1f), color = colors.textMuted)

                        Column(Modifier.weight(1.5f), horizontalAlignment = Alignment.End) {
                            if (node.id to agent in importing) {
                                IndeterminateProgress("Importing ${agentName(agent)} conversations from ${node.name}")
                            } else if (pending > 0) {
                                MyButton(
                                    onClick = {
                                        importing = importing + (node.id to agent)
                                        store.act {
                                            try {
                                                val result = store.api.startImport(ImportRequest(node.id, agent))
                                                skipped = skipped + ((node.id to agent) to result.skipped)
                                                statuses = store.api.imports()
                                                store.refresh()
                                            } finally {
                                                importing = importing - (node.id to agent)
                                            }
                                        }
                                    },
                                    text = "Import $pending",
                                    size = ButtonSize.Sm,
                                    description = "Import $pending ${agentName(agent)} conversations from ${node.name}",
                                )
                            } else {
                                Label("Up to date", color = colors.textMuted)
                            }

                            if (unreadable > 0) {
                                Label("$unreadable could not be read", color = colors.textMuted)
                            }
                        }
                    }

                    Hairline()
                }
            }
        }
    }
}
