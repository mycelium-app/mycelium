package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import kotlinx.coroutines.CancellationException
import mycelium.api.ApiException
import mycelium.api.NodeSummary
import mycelium.api.Project
import mycelium.state.AppState
import mycelium.state.Store
import mycelium.state.agentName

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

suspend fun attempt(block: suspend () -> Unit): String? =
    try {
        block()
        null
    } catch (cancelled: CancellationException) {
        throw cancelled
    } catch (problem: ApiException) {
        problem.message
    } catch (_: Exception) {
        "Could not reach the control plane. Check your connection and try again."
    }

@Composable
fun PlacePicker(store: Store, state: AppState, place: Place, withBranch: Boolean, cwdError: String? = null) {
    val colors = MyTheme.colors
    val nodes = startableNodes(state)
    var projectMenu by remember { mutableStateOf(false) }
    var nodeMenu by remember { mutableStateOf(false) }

    LaunchedEffect(Unit) {
        store.act { store.refreshProjects() }
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
                    text = project?.name ?: "A folder",
                    icon = if (project != null) state.nodes[project.nodeId]?.let(::nodeIcon) else MyIcon.Folder,
                    description = "Project: ${project?.name ?: "A folder"}",
                    variant = ButtonVariant.Outline,
                    trailingIcon = MyIcon.ChevronDown,
                )

                if (projectMenu) {
                    MenuPopup(onDismiss = { projectMenu = false }) {
                        MenuLabel("Work in")

                        for (option in projects) {
                            MenuItem(option.name, onClick = { place.projectId = option.id; projectMenu = false }, checked = option.id == project?.id, leading = state.nodes[option.nodeId]?.let(::nodeIcon), hint = state.nodes[option.nodeId]?.name)
                        }

                        MenuItem("A folder", onClick = { place.projectId = null; projectMenu = false }, checked = project == null, leading = MyIcon.Folder)
                    }
                }
            }
        }
    }

    if (node == null) {
        Label("No node is online. Join a node from Nodes, or wait for one to reconnect.", color = colors.statusFailed)

        return
    }

    if (project == null) {
        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            Label("Node", style = Tokens.Type.MetaStrong, color = colors.textMuted)

            Box {
                MyButton({ nodeMenu = true }, text = node.name, icon = nodeIcon(node), description = "Node: ${node.name}", variant = ButtonVariant.Outline, trailingIcon = MyIcon.ChevronDown)

                if (nodeMenu) {
                    MenuPopup(onDismiss = { nodeMenu = false }) {
                        MenuLabel("Run on")

                        for (option in nodes) {
                            MenuItem(option.name, onClick = { place.nodeId = option.id; nodeMenu = false }, checked = option.id == node.id, leading = nodeIcon(option))
                        }
                    }
                }
            }
        }

        TextInput(place.cwd, { place.cwd = it }, label = "Folder", placeholder = "/home/you/project", mono = true, error = cwdError)
    }

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
        Label("Agent", style = Tokens.Type.MetaStrong, color = colors.textMuted)

        if (agents.isEmpty()) {
            Label("No agent is installed on ${node.name}. Install Claude Code or Codex there, then try again.", color = colors.statusFailed)
        } else {
            Segmented(agents.map { agentName(it.agent) }, agentName(target?.agent.orEmpty()), { picked -> place.agent = agents.first { agentName(it.agent) == picked }.agent }, groupLabel = "Agent")
        }
    }

    if (project != null && withBranch) {
        TextInput(place.branch, { place.branch = it }, label = "New worktree branch (optional)", placeholder = "feature/restore-copy", mono = true)
    }
}
