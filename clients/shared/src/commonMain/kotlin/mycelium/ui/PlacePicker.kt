package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import mycelium.api.NodeSummary
import mycelium.api.Project
import mycelium.state.AppState
import mycelium.state.Store
import mycelium.state.agentName
import mycelium.state.projectName

@Stable
class Place {
    var projectId by mutableStateOf<String?>(null)
    var nodeId by mutableStateOf<String?>(null)
    var agent by mutableStateOf<String?>(null)
    var cwd by mutableStateOf("")
    var branch by mutableStateOf("")
}

data class PlaceTarget(val node: NodeSummary, val agent: String, val project: Project?)

fun Place.target(state: AppState): PlaceTarget? {
    val project = state.projects[projectId]
    val node = if (project != null) state.nodes[project.nodeId] else startableNodes(state).let { nodes -> nodes.firstOrNull { it.id == nodeId } ?: nodes.firstOrNull() }
    val agents = node?.agents?.filter { it.available }.orEmpty()
    val chosen = agents.firstOrNull { it.agent == agent }?.agent ?: agents.firstOrNull()?.agent

    return if (node != null && chosen != null) PlaceTarget(node, chosen, project) else null
}

@Composable
fun NoNodeNotice(color: Color, modifier: Modifier = Modifier) {
    Label("No node is online. Join a node from Nodes, or wait for one to reconnect.", modifier, color = color)
}

@Composable
fun NoAgentNotice(node: NodeSummary) {
    Label("No agent is installed on ${node.name}. Install Claude Code or Codex there, then try again.", color = MyTheme.colors.statusFailed)
}

@Composable
fun NodeChooser(nodes: List<NodeSummary>, node: NodeSummary, onChoose: (String) -> Unit) {
    var open by remember { mutableStateOf(false) }

    Box {
        MyButton({ open = true }, text = node.name, icon = nodeIcon(node), description = "Node: ${node.name}", variant = ButtonVariant.Outline, trailingIcon = MyIcon.ChevronDown)

        if (open) {
            MenuPopup(onDismiss = { open = false }) {
                MenuLabel("Run on")

                for (option in nodes) {
                    MenuItem(option.name, onClick = { onChoose(option.id); open = false }, checked = option.id == node.id, leading = nodeIcon(option))
                }
            }
        }
    }
}

@Composable
fun PlacePicker(store: Store, state: AppState, place: Place, withBranch: Boolean, cwdError: String? = null) {
    val colors = MyTheme.colors
    val nodes = startableNodes(state)
    var projectMenu by remember { mutableStateOf(false) }

    LaunchedEffect(Unit) {
        store.load { store.refreshProjects() }
    }

    val projects = state.projects.values.sortedBy { it.name }
    val target = place.target(state)
    val project = target?.project
    val node = target?.node ?: nodes.firstOrNull()
    val agents = node?.agents?.filter { it.available }.orEmpty()

    if (projects.isNotEmpty()) {
        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            Label("Project", style = Tokens.Type.MetaStrong, color = colors.textMuted)

            Box {
                MyButton(
                    { projectMenu = true },
                    text = project?.name ?: "No project",
                    icon = if (project != null) state.nodes[project.nodeId]?.let(::nodeIcon) else MyIcon.Folder,
                    description = "Project: ${project?.name ?: "No project"}",
                    variant = ButtonVariant.Outline,
                    trailingIcon = MyIcon.ChevronDown,
                )

                if (projectMenu) {
                    ProjectMenu(projects, state.nodes, project?.id, "Work in", onDismiss = { projectMenu = false }) {
                        place.projectId = it
                        projectMenu = false
                    }
                }
            }
        }
    }

    if (node == null) {
        NoNodeNotice(colors.statusFailed)

        return
    }

    if (project == null) {
        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            Label("Node", style = Tokens.Type.MetaStrong, color = colors.textMuted)

            NodeChooser(nodes, node) { place.nodeId = it }
        }

        FolderField(store, node, place.cwd, { place.cwd = it }, label = "Folder", placeholder = "/home/you/project", error = cwdError)
    }

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
        Label("Agent", style = Tokens.Type.MetaStrong, color = colors.textMuted)

        if (agents.isEmpty()) {
            NoAgentNotice(node)
        } else {
            Segmented(agents.map { agentName(it.agent) }, agentName(target?.agent.orEmpty()), { picked -> place.agent = agents.first { agentName(it.agent) == picked }.agent }, groupLabel = "Agent")
        }
    }

    if (project != null && withBranch) {
        TextInput(place.branch, { place.branch = it }, label = "New worktree branch (optional)", placeholder = "feature/restore-copy", mono = true)
    }
}

@Composable
fun ProjectMenu(projects: List<Project>, nodes: Map<String, NodeSummary>, selected: String?, label: String, onDismiss: () -> Unit, setUpFolder: String? = null, onSetUp: () -> Unit = {}, onSelect: (String?) -> Unit) {
    var query by remember { mutableStateOf("") }
    val matching = projects.filter { project ->
        listOf(project.name, project.path, nodes[project.nodeId]?.name.orEmpty()).any { it.contains(query.trim(), ignoreCase = true) }
    }

    val severalNodes = projects.map { it.nodeId }.distinct().size > 1

    MenuPopup(onDismiss = onDismiss) {
        MenuLabel(label)
        TextInput(query, { query = it }, label = "Search projects", placeholder = if (severalNodes) "Name, path or node" else "Name or path", showLabel = false)
        val scroll = rememberLazyListState()

        LazyColumn(Modifier.heightIn(max = 320.dp).scrollbar(scroll), state = scroll) {
            items(matching, key = { it.id }) { option ->
                MenuItem(option.name, onClick = { onSelect(option.id) }, checked = option.id == selected, hint = nodes[option.nodeId]?.name?.takeIf { severalNodes }, leadingContent = { ProjectIcon(option) })
            }

            if (matching.isEmpty()) {
                item { MenuLabel("No matching projects") }
            }
        }

        MenuItem("No project", onClick = { onSelect(null) }, checked = selected == null, leading = MyIcon.Folder)

        if (setUpFolder != null) {
            MenuItem("Set up ${projectName(setUpFolder)} as a project", onClick = onSetUp, leading = MyIcon.Plus)
        }
    }
}
