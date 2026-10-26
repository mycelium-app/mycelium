package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import kotlinx.coroutines.delay
import androidx.compose.ui.unit.dp
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import mycelium.api.ImportJob
import mycelium.api.ImportRequest
import mycelium.api.ImportStatus
import mycelium.state.Store
import mycelium.state.agentName

private val ImportableAgents = listOf("claude", "codex")

private const val ImportPollMillis = 1000L

fun importOutcome(job: ImportJob): String {
    val parts = mutableListOf<String>()
    parts += if (job.imported > 0) "Imported ${job.imported}" else "Nothing new was imported"

    if (job.failed.isNotEmpty()) {
        parts += "${job.failed.size} could not be read"
    }

    if (job.waiting > 0) {
        parts += "${job.waiting} in use, try again later"
    }

    return parts.joinToString(", ")
}

fun pendingImports(status: ImportStatus?): Int = ((status?.found ?: 0) - (status?.imported ?: 0)).coerceAtLeast(0)

@Composable
fun ImportSettings(store: Store) {
    val colors = MyTheme.colors
    val state by store.state.collectAsState()
    var statuses by remember { mutableStateOf<List<ImportStatus>>(emptyList()) }
    var jobs by remember { mutableStateOf<Map<Pair<String, String>, ImportJob>>(emptyMap()) }
    var showingProblems by remember { mutableStateOf<Set<Pair<String, String>>>(emptySet()) }
    val anyRunning = jobs.values.any { it.running }

    LaunchedEffect(Unit) {
        store.act { statuses = store.api.imports() }
        store.act { jobs = store.api.importJobs().associateBy { it.nodeId to it.agent } }
    }

    LaunchedEffect(anyRunning) {
        while (anyRunning) {
            delay(ImportPollMillis)
            store.act {
                jobs = store.api.importJobs().associateBy { it.nodeId to it.agent }

                if (jobs.values.none { it.running }) {
                    statuses = store.api.imports()
                    store.refresh()
                }
            }
        }
    }

    Column(Modifier.widthIn(max = Tokens.Size.Page), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S6)) {
        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            Label("Claude chats", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
            ImportFromClaude(store)
        }

        for (agent in ImportableAgents) {
            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                Label(agentName(agent), Modifier.semantics { heading() }, Tokens.Type.UiStrong)

                val nodes = state.nodes.values.sortedBy { it.name }

                Table(
                    listOf(
                        Col("Node", 120.dp, weight = 1f),
                        Col("Transcripts found", 88.dp, numeric = true, priority = 1),
                        Col("Imported", 72.dp, numeric = true, priority = 1),
                        Col("Status", 140.dp, max = 300.dp, weight = 1f, overflow = Overflow.Wrap),
                        ActionColumn,
                    ),
                    nodes.size,
                    { row, column ->
                        val node = nodes[row]
                        val status = statuses.firstOrNull { it.nodeId == node.id && it.agent == agent }
                        val pending = pendingImports(status)
                        val job = jobs[node.id to agent]

                        when (column) {
                            0 -> TableCell.Text(node.name)
                            1 -> TableCell.Text((status?.found ?: 0).toString(), color = colors.textMuted)
                            2 -> TableCell.Text((status?.imported ?: 0).toString(), color = colors.textMuted)
                            3 -> TableCell.Slot(if (job?.running == true) "Importing ${job.done} of ${job.total}" else if (job != null) importOutcome(job) else if (pending > 0) "$pending ready to import" else "Up to date") {
                                Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                                    if (job?.running == true) {
                                        Label("Importing ${job.done} of ${job.total}", color = colors.textMuted)
                                        DeterminateProgress("Importing ${agentName(agent)} conversations from ${node.name}", if (job.total == 0) 0f else job.done.toFloat() / job.total)
                                    } else if (job != null) {
                                        Label(importOutcome(job), style = Tokens.Type.Meta, color = colors.textMuted)
                                    } else {
                                        Label(if (pending > 0) "$pending ready to import" else "Up to date", color = colors.textMuted)
                                    }
                                }
                            }

                            else -> TableCell.Slot {
                                RowMenu("${agentName(agent)} imports on ${node.name}") { close ->
                                    if (job?.running != true && pending > 0) {
                                        MenuItem("Import $pending conversations", onClick = {
                                            close()
                                            store.act { jobs = jobs + ((node.id to agent) to store.api.startImport(ImportRequest(node.id, agent))) }
                                        })
                                    }

                                    if (job != null && !job.running && job.failed.isNotEmpty()) {
                                        MenuItem(if (node.id to agent in showingProblems) "Hide unreadable conversations" else "Show unreadable conversations", onClick = {
                                            close()
                                            showingProblems = if (node.id to agent in showingProblems) showingProblems - (node.id to agent) else showingProblems + (node.id to agent)
                                        })
                                    }

                                    if (job?.running == true || (pending == 0 && job?.failed.isNullOrEmpty())) {
                                        MenuLabel(if (job?.running == true) "Import in progress" else "No imports waiting")
                                    }
                                }
                            }
                        }
                    },
                    detail = { row ->
                        val node = nodes[row]
                        val job = jobs[node.id to agent]

                        if (job != null && !job.running && node.id to agent in showingProblems) {
                            Column(Modifier.padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                                for (problem in job.failed) {
                                    Label("${problem.path.substringAfterLast('/')}: ${problem.reason}", color = colors.textMuted)
                                }
                            }
                        }
                    },
                )
            }
        }
    }
}
