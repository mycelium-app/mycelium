package mycelium.ui

import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
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
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import kotlinx.coroutines.delay
import mycelium.Platform
import mycelium.api.LoginState
import mycelium.api.NodeSummary
import mycelium.api.ResourceSample
import mycelium.state.AppState
import mycelium.state.Store
import mycelium.state.agentName
import mycelium.state.agentVersion
import mycelium.state.conversationTitle
import mycelium.state.cpuSummary
import mycelium.state.formatBytes
import mycelium.state.isLocalToNode
import mycelium.state.memoryUsedBytes
import mycelium.state.percentOf
import mycelium.state.portLink
import mycelium.state.statusSectionName
import mycelium.state.versionWithUpdate
import kotlin.math.roundToInt

private const val WatchHeartbeatMillis = 15_000L
private val Tabs = listOf("Agents", "Resources", "Ports")

@Composable
fun NodePage(store: Store, state: AppState, nodeId: String, platform: Platform, onBack: () -> Unit, onOpenConversation: (String) -> Unit) {
    val colors = MyTheme.colors
    val node = state.nodes[nodeId]
    var tab by remember { mutableStateOf(Tabs.first()) }

    LaunchedEffect(nodeId, node?.online) {
        while (node?.online == true) {
            store.watchNode(nodeId)
            delay(WatchHeartbeatMillis)
        }
    }

    Column(Modifier.fillMaxSize()) {
        Bar {
            MyButton(onBack, text = "Nodes", muted = true, description = "Back to Nodes")

            if (node == null) {
                Label("This node no longer exists.", color = colors.textMuted)
            } else {
                Label("/", color = colors.textFaint)
                Icon(nodeIcon(node), nodeColour(node.name), Tokens.Size.IconSm)
                Label(node.name, Modifier.semantics { heading() }, Tokens.Type.Title, maxLines = 1)

                if (!LocalTouch.current) {
                    val presence = when {
                        node.draining -> "Draining"
                        node.online -> "Online"
                        else -> "Offline"
                    }

                    Label("$presence  ·  ${node.os} ${node.arch}  ·  ${versionWithUpdate(node.version, node.updateAvailable)}", style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
                }
            }
        }

        if (node == null) {
            return@Column
        }

        TabRow(Tabs, tab, { tab = it }, groupLabel = "Node sections", modifier = Modifier.padding(horizontal = Tokens.Space.S3))

        Hairline()

        Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(Tokens.Space.S4), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
            when (tab) {
                "Agents" -> AgentsTab(node, state, onOpenConversation)
                "Resources" -> ResourcesTab(node, state)
                else -> PortsTab(node, state, platform)
            }
        }
    }
}

@Composable
private fun AgentsTab(node: NodeSummary, state: AppState, onOpenConversation: (String) -> Unit) {
    val installed = node.agents.filter { it.available }
    val sessions = node.sessions.mapNotNull { state.conversations[it] }.sortedByDescending { it.updatedAt }
    val sample = state.resources[node.id]?.latest

    DataTable(
        "Agents",
        listOf(TableColumn("Agent", 1.4f), TableColumn("Version", 1.4f), TableColumn("Signed in")),
        installed.map { agent -> listOf(agentName(agent.agent), versionWithUpdate(agentVersion(agent.version), agent.updateAvailable), node.logins.count { it.agent == agent.agent && it.state == LoginState.SignedIn }.toString()) },
        "No agents installed",
    )

    DataTable(
        "Conversations running here",
        listOf(TableColumn("Conversation", 2f), TableColumn("Agent"), TableColumn("Status"), TableColumn("Folder", 1.5f)),
        sessions.map { listOf(it.title, agentName(it.agent), statusSectionName(it.status), it.cwd) },
        "No agent session is running on this node.",
        onRow = { onOpenConversation(sessions[it].id) },
        rowDescription = { "Open ${sessions[it].title}" },
    )

    DataTable(
        "Agent processes",
        listOf(TableColumn("Process", 1.4f), TableColumn("Conversation", 1.6f), TableColumn("CPU"), TableColumn("Memory")),
        sample?.processes.orEmpty().filter { it.agent }.map { listOf(it.command.ifEmpty { it.name }, state.conversationTitle(it.conversationId).orEmpty(), "${it.cpuPercent.roundToInt()}%", formatBytes(it.memoryBytes)) },
        "Agent processes appear when this node reports its resources.",
    )
}

@Composable
@OptIn(ExperimentalLayoutApi::class)
private fun ResourcesTab(node: NodeSummary, state: AppState) {
    val colors = MyTheme.colors
    val resources = state.resources[node.id]
    val sample = readableSample(node, resources?.latest) ?: return

    FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
        val cpu = sample.cpuPercent.roundToInt()
        val memoryPercent = percentOf(memoryUsedBytes(sample), sample.memoryTotalBytes)

        Kpi("CPU", "$cpu%", kpiSlot(), caption = "${sample.cores} cores", meterPercent = cpu.coerceIn(0, 100))
        Kpi(
            "Memory",
            "${formatBytes(memoryUsedBytes(sample))} of ${formatBytes(sample.memoryTotalBytes)}",
            kpiSlot(),
            caption = if (sample.swapTotalBytes > 0) "Swap ${formatBytes(sample.swapTotalBytes - sample.swapFreeBytes)} of ${formatBytes(sample.swapTotalBytes)}" else null,
            meterPercent = memoryPercent,
        )

        for (disk in sample.disks) {
            Kpi("Disk ${disk.mount}", "${formatBytes(disk.freeBytes)} free", kpiSlot(), caption = "of ${formatBytes(disk.totalBytes)}", meterPercent = 100 - percentOf(disk.freeBytes, disk.totalBytes))
        }

        if (sample.load.size == 3) {
            Kpi("Load", sample.load[0].toString(), kpiSlot(), caption = "5 min ${sample.load[1]}  ·  15 min ${sample.load[2]}")
        }
    }

    val history = resources?.history.orEmpty()

    Column(Modifier.fillMaxWidth().border(Tokens.Size.Hairline, colors.border, BlockShape).background(colors.bgPanel, BlockShape).padding(Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        Label("CPU", Modifier.semantics { heading() }, Tokens.Type.UiStrong)

        val summary = cpuSummary(history)
        val minutes = if (history.size > 1) ((history.last().atMs - history.first().atMs) / 60_000).coerceAtLeast(1) else 0

        if (summary == null || history.size < 2) {
            Label("The chart appears after a few samples.", style = Tokens.Type.Meta, color = colors.textMuted)
        } else {
            LineChart(history, "CPU over the last $minutes minutes. $summary.")
            Label("Last $minutes minutes  ·  $summary", style = Tokens.Type.Meta, color = colors.textMuted)
        }
    }

    DataTable(
        "Busiest processes",
        listOf(TableColumn("Process", 2f), TableColumn("CPU"), TableColumn("Memory")),
        sample.processes.map { process ->
            val conversation = state.conversationTitle(process.conversationId)

            listOf(listOfNotNull(process.command.ifEmpty { process.name }, conversation).joinToString(" · "), "${process.cpuPercent.roundToInt()}%", formatBytes(process.memoryBytes))
        },
        "No processes reported.",
    )
}

@Composable
private fun PortsTab(node: NodeSummary, state: AppState, platform: Platform) {
    val sample = readableSample(node, state.resources[node.id]?.latest) ?: return

    DataTable(
        "Listening ports",
        listOf(TableColumn("Port", 0.7f), TableColumn("Address"), TableColumn("Process"), TableColumn("Conversation", 1.6f)),
        sample.ports.map { port -> listOf(port.port.toString(), port.address, port.process.orEmpty(), state.conversationTitle(port.conversationId).orEmpty()) },
        "No process is listening on this node.",
        trailing = { index ->
            val port = sample.ports[index]
            val link = portLink(node, port)

            if (link != null) {
                MyButton({ platform.openUrl(link) }, text = "Open", size = ButtonSize.Sm, variant = ButtonVariant.Outline, description = "Open port ${port.port} on ${node.name} in the browser")
            } else if (isLocalToNode(port)) {
                Label("Local only", style = Tokens.Type.Meta, color = MyTheme.colors.textMuted, maxLines = 1)
            }
        },
    )
}

@Composable
private fun readableSample(node: NodeSummary, sample: ResourceSample?): ResourceSample? {
    val notice = when {
        !node.online -> "This node is offline. Resources and ports appear here when it connects."
        sample == null -> "Waiting for the first sample from ${node.name}."
        !sample.available -> "Resources are not available on this node. Mycelium reads them from /proc, which only Linux provides."
        else -> null
    }

    if (notice != null) {
        Label(notice, color = MyTheme.colors.textMuted)
    }

    return sample.takeIf { notice == null }
}
