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
import androidx.compose.ui.unit.dp
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import mycelium.api.ImportRequest
import mycelium.api.ImportStatus
import mycelium.api.UsageLimitSetting
import mycelium.state.Store
import mycelium.state.agentName

private val ImportableAgents = listOf("claude", "codex")

private val UsageLimitChoices = listOf("Ask me" to "ask", "Wait and resume automatically" to "resume")

fun pendingImports(status: ImportStatus?): Int = ((status?.found ?: 0) - (status?.imported ?: 0)).coerceAtLeast(0)

@Composable
fun AgentsSettings(store: Store) {
    val colors = MyTheme.colors
    val state by store.state.collectAsState()
    var statuses by remember { mutableStateOf<List<ImportStatus>>(emptyList()) }
    var skipped by remember { mutableStateOf<Map<Pair<String, String>, Int>>(emptyMap()) }
    var importing by remember { mutableStateOf<Set<Pair<String, String>>>(emptySet()) }

    var onUsageLimit by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(Unit) {
        store.act { statuses = store.api.imports() }
        store.act { onUsageLimit = store.api.usageLimitSetting().onUsageLimit }
    }

    Column(Modifier.widthIn(max = Tokens.Size.Page), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S6)) {
        onUsageLimit?.let { chosen ->
            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                Label("When a usage limit is reached", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
                Segmented(
                    UsageLimitChoices.map { it.first },
                    UsageLimitChoices.first { it.second == chosen }.first,
                    { label ->
                        val picked = UsageLimitChoices.first { it.first == label }.second
                        onUsageLimit = picked
                        store.act { store.api.saveUsageLimitSetting(UsageLimitSetting(picked)) }
                    },
                    groupLabel = "When a usage limit is reached",
                )
            }
        }

        for (agent in ImportableAgents) {
            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                Label(agentName(agent), Modifier.semantics { heading() }, Tokens.Type.UiStrong)

                ContentTable(listOf(ColumnSpec(max = 320.dp, flexible = true), ColumnSpec(), ColumnSpec(), ColumnSpec(max = 200.dp))) {
                    ContentRow(header = true) {
                        Label("Node", style = Tokens.Type.MetaStrong, color = colors.textMuted)
                        Label("Transcripts found", style = Tokens.Type.MetaStrong, color = colors.textMuted)
                        Label("Imported", style = Tokens.Type.MetaStrong, color = colors.textMuted)
                        Box(Modifier)
                    }

                    Hairline()

                    for (node in state.nodes.values.sortedBy { it.name }) {
                        val status = statuses.firstOrNull { it.nodeId == node.id && it.agent == agent }
                        val pending = pendingImports(status)
                        val unreadable = skipped[node.id to agent] ?: 0

                        ContentRow {
                            Label(node.name, maxLines = 1)
                            Label((status?.found ?: 0).toString(), color = colors.textMuted)
                            Label((status?.imported ?: 0).toString(), color = colors.textMuted)

                            Column(horizontalAlignment = Alignment.End) {
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
}
