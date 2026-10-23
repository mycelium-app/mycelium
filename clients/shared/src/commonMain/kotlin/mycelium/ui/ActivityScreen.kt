package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import mycelium.api.Activity
import mycelium.api.ActivityRow
import mycelium.state.ActivityRanges
import mycelium.state.AppState
import mycelium.state.Store
import mycelium.state.agentName
import mycelium.state.dayLabel
import mycelium.state.formatCost
import mycelium.state.formatDuration
import mycelium.state.formatTokens
import mycelium.state.tokensPerDaySummary
import mycelium.state.usageNotice

private val Columns = listOf(TableColumn("", 2f), TableColumn("Turns", 0.7f), TableColumn("Tokens", 0.9f), TableColumn("API equivalent", 1.1f), TableColumn("Time working", 1.1f))

private fun rowCells(row: ActivityRow, name: String = row.name): List<String> =
    listOf(name, row.turns.toString(), if (row.reportedTurns == 0L) "Not reported" else formatTokens(row.tokens), formatCost(row.costUsd), formatDuration(row.workingMs))

@Composable
@OptIn(ExperimentalLayoutApi::class)
fun ActivityScreen(store: Store, state: AppState, onOpenConversation: (String) -> Unit) {
    val colors = MyTheme.colors
    var range by remember { mutableStateOf(ActivityRanges[1].second) }
    var nodeId by remember { mutableStateOf<String?>(null) }
    var nodeMenu by remember { mutableStateOf(false) }
    var activity by remember { mutableStateOf<Activity?>(null) }
    val latestChange = state.conversations.values.maxOfOrNull { it.updatedAt }
    val nodes = state.nodes.values.sortedBy { it.name }

    LaunchedEffect(range, nodeId, latestChange) {
        store.act { activity = store.api.activity(range, nodeId) }
    }

    Column(Modifier.fillMaxSize()) {
        Bar {
            BarTitle("Activity")
            Box(Modifier.weight(1f))
            Segmented(ActivityRanges.map { it.first }, ActivityRanges.first { it.second == range }.first, { label -> range = ActivityRanges.first { it.first == label }.second }, groupLabel = "Time range")

            Box {
                MyButton({ nodeMenu = true }, text = nodes.firstOrNull { it.id == nodeId }?.name ?: "All nodes", trailingIcon = MyIcon.ChevronDown, variant = ButtonVariant.Outline, description = "Choose nodes")

                if (nodeMenu) {
                    MenuPopup(onDismiss = { nodeMenu = false }) {
                        MenuItem("All nodes", onClick = { nodeMenu = false; nodeId = null }, checked = nodeId == null)

                        for (node in nodes) {
                            MenuItem(node.name, onClick = { nodeMenu = false; nodeId = node.id }, checked = nodeId == node.id, leading = nodeIcon(node))
                        }
                    }
                }
            }
        }

        val loaded = activity

        if (loaded == null) {
            Label("Loading activity.", Modifier.padding(Tokens.Space.S4), color = colors.textMuted)

            return@Column
        }

        Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(Tokens.Space.S4), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
            if (loaded.totals.turns == 0L) {
                Label("No turns in this range yet.", color = colors.textMuted)

                return@Column
            }

            FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
                Kpi("Tokens", if (loaded.totals.reportedTurns == 0L) "Not reported" else formatTokens(loaded.totals.tokens), kpiSlot(), caption = "${loaded.totals.turns} turns")
                Kpi("API equivalent", formatCost(loaded.totals.costUsd), kpiSlot(), caption = "as the agents report it, not billed on a subscription")
                Kpi("Time working", formatDuration(loaded.totals.workingMs), kpiSlot(), caption = "${formatDuration(loaded.totals.workingMs / loaded.totals.turns)} a turn on average")
            }

            usageNotice(loaded)?.let { Label(it, color = colors.textMuted) }

            if (loaded.range != "today") {
                Column(Modifier.fillMaxWidth().border(Tokens.Size.Hairline, colors.border, BlockShape).background(colors.bgPanel, BlockShape).padding(Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                    Label("Tokens per day", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
                    BarChart(loaded.days.map { it.tokens }, "Tokens per day over ${loaded.days.size} days. ${tokensPerDaySummary(loaded)}")
                    Label(tokensPerDaySummary(loaded), style = Tokens.Type.Meta, color = colors.textMuted)
                }
            }

            val busyDays = loaded.days.filter { it.turns > 0 }.reversed()

            DataTable("Days", Columns.withTitle("Day"), busyDays.map { rowCells(it, dayLabel(it.key)) }, "No turns on these days.")
            DataTable(
                "Conversations",
                Columns.withTitle("Conversation"),
                loaded.conversations.map { rowCells(it) },
                "No conversations in this range.",
                onRow = { onOpenConversation(loaded.conversations[it].key) },
                rowDescription = { "Open ${loaded.conversations[it].name}" },
            )
            DataTable("Projects", Columns.withTitle("Project"), loaded.projects.map { rowCells(it) }, "No projects in this range.")
            DataTable("Nodes", Columns.withTitle("Node"), loaded.nodes.map { rowCells(it) }, "No nodes in this range.")
            DataTable("Agents", Columns.withTitle("Agent"), loaded.agents.map { rowCells(it, agentName(it.name)) }, "No agents in this range.")
            DataTable("Models", Columns.withTitle("Model"), loaded.models.map { rowCells(it) }, "No models in this range.")
        }
    }
}

private fun List<TableColumn>.withTitle(title: String): List<TableColumn> = listOf(first().copy(title = title)) + drop(1)
