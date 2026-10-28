package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import mycelium.api.Activity
import mycelium.api.ActivityRow
import mycelium.state.ActivityRanges
import mycelium.state.AppState
import mycelium.state.Store
import mycelium.state.EstimateNote
import mycelium.state.agentName
import mycelium.state.estimateUsage
import mycelium.state.formatCo2
import mycelium.state.formatEnergy
import mycelium.state.formatTreeDays
import mycelium.state.formatWater
import mycelium.state.dayLabel
import mycelium.state.inGroup
import mycelium.state.formatCost
import mycelium.state.formatDuration
import mycelium.state.formatTokens
import mycelium.state.tokensPerHourSummary
import mycelium.state.tokensPerHourTitle
import mycelium.state.usageNotice

private val Columns = listOf(TableColumn("", 2f), TableColumn("Turns", 0.7f, numeric = true), TableColumn("Tokens", 0.9f, numeric = true), TableColumn("API equivalent", 1.1f, numeric = true), TableColumn("Time working", 1.1f, numeric = true))

private val CompactColumns = listOf(TableColumn("", 2f), TableColumn("Tokens", 0.9f, numeric = true), TableColumn("API equivalent", 1.1f, numeric = true))

private val SideBySideWidth = 600.dp

private fun compactCells(row: ActivityRow): List<String> = listOf(row.name, if (row.reportedTurns == 0L) "Not reported" else formatTokens(row.tokens), formatCost(row.costUsd))

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
    val groupFilter = LocalGroupFilter.current

    LaunchedEffect(range, nodeId, latestChange) {
        store.act { activity = store.api.activity(range, nodeId) }
    }

    Column(Modifier.fillMaxSize()) {
        Bar {
            OpenListButton()
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

        Column(Modifier.weight(1f).fillMaxWidth().verticalScrollWithBar().padding(Tokens.Space.S4), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
            if (loaded.totals.turns == 0L) {
                Label("No turns in this range yet.", color = colors.textMuted)

                return@Column
            }

            val estimate = estimateUsage(loaded.totals)
            val reported = loaded.totals.reportedTurns > 0

            FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
                Kpi("Tokens", if (reported) formatTokens(loaded.totals.tokens) else "Not reported", kpiSlot(), caption = "${loaded.totals.turns} turns, ${formatDuration(loaded.totals.workingMs)} working")
                Kpi("API equivalent", formatCost(loaded.totals.costUsd), kpiSlot(), caption = if (loaded.totals.costEstimated) "partly estimated from list prices, not billed on a subscription" else "as the agents report it, not billed on a subscription")
                Kpi("Energy and water", if (reported) formatEnergy(estimate.energyWh) else "Not reported", kpiSlot(), caption = if (reported) formatWater(estimate.waterMl) else null)
                Kpi("Forest", if (reported) formatTreeDays(estimate.treeDays) else "Not reported", kpiSlot(), caption = if (reported) formatCo2(estimate.co2Grams) else null)
            }

            Label(EstimateNote, style = Tokens.Type.Meta, color = colors.textMuted)

            usageNotice(loaded)?.let { Label(it, color = colors.textMuted) }

            Column(Modifier.fillMaxWidth().border(Tokens.Size.Hairline, colors.border, BlockShape).background(colors.bgPanel, BlockShape).padding(Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                Label(tokensPerHourTitle(loaded.range), Modifier.semantics { heading() }, Tokens.Type.Meta, colors.textMuted)
                BarChart((if (loaded.range == "all") loaded.days else loaded.hours).map { it.tokens }, "${tokensPerHourTitle(loaded.range)}. ${tokensPerHourSummary(loaded)}")
                Label(tokensPerHourSummary(loaded), style = Tokens.Type.Meta, color = colors.textMuted)
            }

            val shownProjects = loaded.projects.filter { inGroup(state.projects[it.key]?.groupId, groupFilter) }
            val shownConversations = loaded.conversations.filter { inGroup(state.conversations[it.key]?.groupId, groupFilter) }
            val projects = @Composable { modifier: Modifier -> DataTable("Projects", CompactColumns.withTitle("Project"), shownProjects.map(::compactCells), "No projects in this range.", modifier) }
            val nodeTable = @Composable { modifier: Modifier -> DataTable("Nodes", CompactColumns.take(2).withTitle("Node"), loaded.nodes.map { compactCells(it).take(2) }, "No nodes in this range.", modifier) }

            BoxWithConstraints(Modifier.fillMaxWidth()) {
                if (maxWidth >= SideBySideWidth) {
                    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
                        projects(Modifier.weight(1f))
                        nodeTable(Modifier.weight(1f))
                    }
                } else {
                    Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
                        projects(Modifier.fillMaxWidth())
                        nodeTable(Modifier.fillMaxWidth())
                    }
                }
            }

            val busyDays = loaded.days.filter { it.turns > 0 }.reversed()

            DataTable("Days", Columns.withTitle("Day"), busyDays.map { rowCells(it, dayLabel(it.key)) }, "No turns on these days.")
            DataTable(
                "Conversations",
                Columns.withTitle("Conversation"),
                shownConversations.map { rowCells(it) },
                "No conversations in this range.",
                onRow = { onOpenConversation(shownConversations[it].key) },
                rowDescription = { "Open ${shownConversations[it].name}" },
            )
            DataTable("Agents", Columns.withTitle("Agent"), loaded.agents.map { rowCells(it, agentName(it.name)) }, "No agents in this range.")
            DataTable("Models", Columns.withTitle("Model").let { listOf(it.first().copy(rigid = true)) + it.drop(1) }, loaded.models.map { rowCells(it) }, "No models in this range.")
        }
    }
}

private fun List<TableColumn>.withTitle(title: String): List<TableColumn> = listOf(first().copy(title = title)) + drop(1)
