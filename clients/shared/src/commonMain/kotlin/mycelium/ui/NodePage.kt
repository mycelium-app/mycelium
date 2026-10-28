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
import kotlin.time.Clock
import mycelium.Platform
import mycelium.api.LoginState
import mycelium.api.NodeSummary
import mycelium.api.ProcessUsage
import mycelium.api.ResourceSample
import mycelium.state.AppState
import mycelium.state.Store
import mycelium.state.agentName
import mycelium.state.agentVersion
import mycelium.state.conversationTitle
import mycelium.state.compareVersions
import mycelium.state.environmentCannotRunText
import mycelium.state.environmentControl
import mycelium.state.environmentRunningFor
import mycelium.state.environmentVersionsText
import mycelium.state.cpuSummary
import mycelium.state.formatBytes
import mycelium.state.isLocalToNode
import mycelium.state.memoryUsedBytes
import mycelium.state.percentOf
import mycelium.state.portLink
import mycelium.state.statusSectionName
import mycelium.state.versionParts
import mycelium.state.versionWithUpdate
import kotlin.math.roundToInt

private const val WatchHeartbeatMillis = 15_000L
private val Tabs = listOf("Agents", "Resources", "Ports")

@Composable
fun NodePage(store: Store, state: AppState, nodeId: String, platform: Platform, onBack: () -> Unit, onOpenConversation: (String) -> Unit) {
    val colors = MyTheme.colors
    val node = state.nodes[nodeId]
    var tab by remember { mutableStateOf(Tabs.first()) }
    var controlVersion by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(Unit) {
        attempt { controlVersion = store.api.version() }
    }

    LaunchedEffect(nodeId, node?.online) {
        while (node?.online == true) {
            store.watchNode(nodeId)
            delay(WatchHeartbeatMillis)
        }
    }

    Column(Modifier.fillMaxSize()) {
        Bar {
            BackButton("Nodes", onBack, showText = true)

            if (node == null) {
                Label("This node no longer exists.", color = colors.textMuted)
            } else {
                Label("/", color = colors.textFaint)
                Icon(nodeIcon(node), nodeColour(node), Tokens.Size.IconSm)
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

        val updateTo = controlVersion?.takeIf { node.online && isOlderThan(node.version, it) }?.let { "v${it.removePrefix("v")}" }

        if (updateTo != null || node.updateMessage != null) {
            Row(Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3), verticalAlignment = Alignment.CenterVertically) {
                if (updateTo != null) {
                    MyButton({ store.act { store.api.updateNodeSoftware(node.id) } }, text = "Update to $updateTo", variant = ButtonVariant.Outline)
                }

                node.updateMessage?.let { Label(it, style = Tokens.Type.Meta, color = colors.textMuted) }
            }
        }

        TabRow(Tabs, tab, { tab = it }, groupLabel = "Node sections", modifier = Modifier.padding(horizontal = Tokens.Space.S3))

        Hairline()

        Column(Modifier.weight(1f).fillMaxWidth().verticalScrollWithBar().padding(Tokens.Space.S4), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
            when (tab) {
                "Agents" -> AgentsTab(node, state, onOpenConversation)
                "Resources" -> ResourcesTab(store, node, state, onOpenConversation)
                else -> PortsTab(node, state, platform)
            }
        }
    }
}

private fun isOlderThan(version: String, other: String): Boolean {
    val own = versionParts(version) ?: return false
    val wanted = versionParts(other) ?: return false

    return compareVersions(own, wanted) < 0
}

@Composable
private fun AgentsTab(node: NodeSummary, state: AppState, onOpenConversation: (String) -> Unit) {
    val installed = node.agents.filter { it.available }
    val sessions = node.sessions.mapNotNull { state.conversations[it] }.sortedByDescending { it.updatedAt }
    val sample = state.resources[node.id]?.latest

    DataTable(
        "Agents",
        listOf(TableColumn("Agent", 1.4f), TableColumn("Version", 1.4f), TableColumn("Signed in", numeric = true)),
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
        listOf(TableColumn("Process", 1.4f), TableColumn("Conversation", 1.6f), TableColumn("CPU", numeric = true), TableColumn("Memory", numeric = true)),
        sample?.processes.orEmpty().filter { it.agent }.map { listOf(it.command.ifEmpty { it.name }, state.conversationTitle(it.conversationId).orEmpty(), "${it.cpuPercent.roundToInt()}%", formatBytes(it.memoryBytes)) },
        "Agent processes appear when this node reports its resources.",
    )
}

@Composable
@OptIn(ExperimentalLayoutApi::class)
private fun ResourcesTab(store: Store, node: NodeSummary, state: AppState, onOpenConversation: (String) -> Unit) {
    val colors = MyTheme.colors
    var stopping by remember { mutableStateOf<ProcessUsage?>(null) }
    var stopped by remember { mutableStateOf<String?>(null) }
    val resources = state.resources[node.id]
    val sample = readableSample(node, resources?.latest)

    if (sample == null) {
        EnvironmentsSection(store, node, state, onOpenConversation)

        return
    }

    FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
        val cpu = sample.cpuPercent.roundToInt()
        val memoryPercent = percentOf(memoryUsedBytes(sample), sample.memoryTotalBytes)

        Kpi("CPU", "$cpu%", kpiSlot(), caption = listOfNotNull("${sample.cores} cores", sample.load.firstOrNull()?.let { "load $it" }).joinToString("  ·  "), meterPercent = cpu.coerceIn(0, 100))
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

        val docker = sample.docker

        when {
            docker == null -> Kpi("Docker", "Not available", kpiSlot(), caption = "Docker is not installed on this node, or its service is not running.")
            docker.containers.isEmpty() -> Kpi("Docker", "No containers", kpiSlot())
            else -> Kpi("Docker", "${docker.containers.size} ${if (docker.containers.size == 1) "container" else "containers"}", kpiSlot(), caption = docker.reclaimableBytes?.let { "${formatBytes(it)} reclaimable" } ?: "${docker.running} running")
        }
    }

    EnvironmentsSection(store, node, state, onOpenConversation)

    val history = resources?.history.orEmpty()

    Column(Modifier.fillMaxWidth().border(Tokens.Size.Hairline, colors.border, BlockShape).background(colors.bgPanel, BlockShape).padding(Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        val summary = cpuSummary(history)
        val minutes = if (history.size > 1) ((history.last().atMs - history.first().atMs) / 60_000).coerceAtLeast(1) else 0

        Label(if (minutes > 0) "CPU, last $minutes minutes" else "CPU", Modifier.semantics { heading() }, Tokens.Type.Meta, colors.textMuted)

        if (summary == null || history.size < 2) {
            Label("The chart appears after a few samples.", style = Tokens.Type.Meta, color = colors.textMuted)
        } else {
            BarChart(history.map { it.cpuPercent.roundToInt().toLong() }, "CPU over the last $minutes minutes. $summary.", ceiling = 100)
            Label(summary, style = Tokens.Type.Meta, color = colors.textMuted)
        }
    }

    val processNames = sample.processes.map { process -> listOfNotNull(process.command.ifEmpty { process.name }, state.conversationTitle(process.conversationId)).joinToString(" · ") }

    DataTable(
        "Busiest processes",
        listOf(TableColumn("Process", 2f), TableColumn("CPU", numeric = true), TableColumn("Memory", numeric = true)),
        sample.processes.mapIndexed { index, process -> listOf(processNames[index], "${process.cpuPercent.roundToInt()}%", formatBytes(process.memoryBytes)) },
        "No processes reported.",
        trailing = { index ->
            if (sample.processes[index].stoppable) {
                MyButton({ stopping = sample.processes[index] }, text = "Stop", size = ButtonSize.Sm, muted = true, description = "Stop ${processNames[index]}")
            }
        },
        monospaceColumns = setOf(0),
    )

    stopped?.let { Label(it, color = colors.textMuted) }

    if (sample.docker != null && sample.docker.containers.isNotEmpty()) {
        DataTable(
            "Docker containers",
            listOf(TableColumn("Container", 2f), TableColumn("State")),
            sample.docker.containers.map { listOf(it.name, it.state) },
            "No containers on this node.",
            monospaceColumns = setOf(0),
        )
    }

    stopping?.let { process ->
        val name = process.command.ifEmpty { process.name }

        Dialog(onDismiss = { stopping = null }) {
            Label("Stop $name?", Modifier.semantics { heading() }, Tokens.Type.Title)
            Label("Mycelium asks $name (process ${process.pid}) on ${node.name} to shut down. Work it has not saved may be lost, and a process that ignores the request keeps running.", style = Tokens.Type.Meta, color = colors.textMuted)

            FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                MyButton({ stopping = null }, text = "Cancel", muted = true)
                MyButton(
                    {
                        stopping = null
                        store.act {
                            store.api.stopProcess(node.id, process.pid)
                            stopped = "Asked $name to stop. It leaves this list once it has exited."
                        }
                    },
                    text = "Stop $name",
                    variant = ButtonVariant.Danger,
                )
            }
        }
    }
}

@Composable
private fun EnvironmentsSection(store: Store, node: NodeSummary, state: AppState, onOpenConversation: (String) -> Unit) {
    val colors = MyTheme.colors
    val capability = node.environment ?: return

    if (capability.available) {
        val now = Clock.System.now()
        val running = capability.running.filter { state.conversations[it.conversationId] != null }
        val titles = running.map { state.conversations.getValue(it.conversationId).title }

        DataTable(
            "Environments",
            listOf(TableColumn("Conversation", 2.4f), TableColumn("Control", 1.2f), TableColumn("Running", numeric = true), TableColumn("Memory", numeric = true)),
            running.mapIndexed { index, environment -> listOf(titles[index], environmentControl(environment.controller, null), environmentRunningFor(environment.startedAt, now), environment.memoryBytes?.let(::formatBytes).orEmpty()) },
            "No environment is running on this node.",
            onRow = { onOpenConversation(running[it].conversationId) },
            rowDescription = { "Open ${titles[it]}" },
            trailing = { index ->
                MyButton({ store.stopEnvironment(running[index].conversationId) }, text = "Stop", size = ButtonSize.Sm, muted = true, description = "Stop the environment of ${titles[index]}")
            },
            status = "Can run environments  ·  ${environmentVersionsText(capability.versions)}",
        )

        return
    }

    Column(Modifier.fillMaxWidth().border(Tokens.Size.Hairline, colors.border, BlockShape).background(colors.bgPanel, BlockShape)) {
        Label("Environments", Modifier.padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2).semantics { heading() }, Tokens.Type.UiStrong)
        Hairline()

        Column(Modifier.padding(Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            StatusMark(environmentCannotRunText(capability), Tone.Attention, MyIcon.TriangleAlert)

            if (capability.missing.isNotEmpty()) {
                Label("Install them with the package manager on ${node.name}, then check again.", color = colors.textMuted)
                MyButton({ store.checkEnvironment(node.id) }, text = "Check again", variant = ButtonVariant.Outline)
            }
        }
    }
}

@Composable
private fun PortsTab(node: NodeSummary, state: AppState, platform: Platform) {
    val sample = readableSample(node, state.resources[node.id]?.latest) ?: return

    DataTable(
        "Listening ports",
        listOf(TableColumn("Port", 0.7f, numeric = true), TableColumn("Address"), TableColumn("Process"), TableColumn("Conversation", 1.6f)),
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
