package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.runtime.setValue
import mycelium.api.Forge
import mycelium.api.NodeSummary
import mycelium.api.Project
import mycelium.state.AppState
import mycelium.state.Store
import mycelium.state.agentName
import mycelium.state.projectName

@Stable
class Place {
    var projectId by mutableStateOf<String?>(null)
    var checkedOut by mutableStateOf<Project?>(null)
    var nodeId by mutableStateOf<String?>(null)
    var agent by mutableStateOf<String?>(null)
    var cwd by mutableStateOf("")
    var branch by mutableStateOf("")
}

data class PlaceTarget(val node: NodeSummary, val agent: String, val project: Project?)

fun Place.target(state: AppState): PlaceTarget? {
    val project = resolveProject(state, projectId, checkedOut)
    val nodes = startableNodes(state)
    val node = if (project != null) runOnNode(project, nodes, state.conversations.values.sortedByDescending { it.updatedAt }, nodeId) else nodes.firstOrNull { it.id == nodeId } ?: nodes.firstOrNull()
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
fun NodeChooser(nodes: List<NodeSummary>, node: NodeSummary?, cloneOn: Set<String> = emptySet(), onChoose: (String) -> Unit) {
    var open by remember { mutableStateOf(false) }

    Box {
        MyButton({ open = true }, text = node?.name ?: "Choose a node", icon = node?.let(::nodeIcon) ?: MyIcon.Server, iconColour = node?.let { nodeColour(it) }, description = "Node: ${node?.name ?: "choose a node"}", variant = ButtonVariant.Outline, trailingIcon = MyIcon.ChevronDown)

        if (open) {
            MenuPopup(onDismiss = { open = false }) {
                MenuLabel("Run on")

                for (option in nodes) {
                    MenuItem(if (option.id in cloneOn) "Clone onto ${option.name}" else option.name, onClick = { onChoose(option.id); open = false }, checked = option.id == node?.id, leading = nodeIcon(option), leadingColour = nodeColour(option))
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
    val project = resolveProject(state, place.projectId, place.checkedOut)
    val projectNode = project?.let { held -> runOnNode(held, nodes, state.conversations.values.sortedByDescending { it.updatedAt }, place.nodeId) }
    val choosingNode = project != null && nodes.isNotEmpty() && projectNode == null
    val target = place.target(state)
    val node = projectNode ?: nodes.firstOrNull { it.id == place.nodeId }?.takeIf { project == null } ?: nodes.firstOrNull()
    val agents = node?.agents?.filter { it.available }.orEmpty()

    if (projects.isNotEmpty()) {
        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            Label("Project", style = Tokens.Type.MetaStrong, color = colors.textMuted)

            Box {
                MyButton(
                    { projectMenu = true },
                    text = project?.name ?: "No project",
                    icon = MyIcon.Folder,
                    description = "Project: ${project?.name ?: "No project"}",
                    variant = ButtonVariant.Outline,
                    trailingIcon = MyIcon.ChevronDown,
                )

                if (projectMenu) {
                    ProjectMenu(projects, state.nodes, project?.id, "Work in", onDismiss = { projectMenu = false }, showNodes = false) {
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

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
        Label("Node", style = Tokens.Type.MetaStrong, color = colors.textMuted)

        val cloneOn = project?.takeIf { cloneable(it) }?.let { held -> nodes.filter { held.checkoutOn(it.id) == null }.map { it.id }.toSet() }.orEmpty()

        NodeChooser(nodes, node.takeIf { !choosingNode }, cloneOn) { chosenId ->
            when {
                project == null -> place.nodeId = chosenId
                project.checkoutOn(chosenId) != null -> place.nodeId = chosenId
                cloneable(project) -> store.act {
                    val checkout = store.addCheckout(project.id, chosenId)

                    place.checkedOut = checkout
                    place.projectId = checkout.id
                    place.nodeId = chosenId
                }
                else -> {
                    place.projectId = null
                    place.nodeId = chosenId
                }
            }
        }

        if (choosingNode) {
            Label("Choose a node to run on.", color = colors.statusFailed)
        }
    }

    if (project == null) {
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

data class NewProjectDraft(val name: String, val repository: Boolean = false, val owner: String? = null, val forgeId: String? = null)

fun NewProjectDraft.forge(forges: List<Forge>): Forge? = forges.firstOrNull { it.id == forgeId } ?: forges.firstOrNull()

fun NewProjectDraft.ownerOn(forge: Forge): String = owner?.takeIf { it in listOf(forge.account) + forge.organisations } ?: forge.account

class NewProjectOption(
    val draft: NewProjectDraft,
    val chosen: Boolean,
    val nodeName: String?,
    val forges: List<Forge>,
    val onChoose: () -> Unit,
    val onChange: (NewProjectDraft) -> Unit,
    val onSubmit: () -> Unit,
)

@Composable
private fun NewProjectForm(option: NewProjectOption) {
    val draft = option.draft
    var ownerMenu by remember { mutableStateOf(false) }
    var forgeMenu by remember { mutableStateOf(false) }

    Column(Modifier.padding(start = Tokens.Space.S8, end = Tokens.Space.S2, top = Tokens.Space.S1, bottom = Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        TextInput(draft.name, { option.onChange(draft.copy(name = it)) }, label = "Name", mono = true, onSubmit = option.onSubmit)

        Label("~/projects/${draft.name.trim()}", style = Tokens.Type.Code, color = MyTheme.colors.textMuted)

        draft.forge(option.forges)?.let { forge ->
            val host = forge.baseUrl.substringAfter("://")
            val owner = draft.ownerOn(forge)
            val severalForges = option.forges.size > 1
            val question = if (severalForges) "Also create a repository on" else "Also create a repository on $host"

            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                Checkbox(draft.repository, description = question, onToggle = { option.onChange(draft.copy(repository = !draft.repository)) }, checkedState = "Ticked", uncheckedState = "Not ticked")

                Label(question)

                if (severalForges) {
                    Box {
                        MyButton({ forgeMenu = true }, text = forgeProduct(forge), icon = forgeIcon(forge), description = "Forge: ${forgeProduct(forge)}", variant = ButtonVariant.Outline, size = ButtonSize.Sm, muted = true, trailingIcon = MyIcon.ChevronDown)

                        if (forgeMenu) {
                            MenuPopup(onDismiss = { forgeMenu = false }) {
                                MenuLabel("Forge")

                                for (choice in option.forges) {
                                    MenuItem(forgeProduct(choice), onClick = { option.onChange(draft.copy(forgeId = choice.id)); forgeMenu = false }, checked = choice.id == forge.id, hint = choice.baseUrl.substringAfter("://"), leading = forgeIcon(choice))
                                }
                            }
                        }
                    }
                }
            }

            if (draft.repository) {
                Box {
                    MyButton({ ownerMenu = true }, text = "$owner / ${draft.name.trim()}", description = "Owner: $owner", variant = ButtonVariant.Outline, size = ButtonSize.Sm, muted = true, trailingIcon = MyIcon.ChevronDown)

                    if (ownerMenu) {
                        MenuPopup(onDismiss = { ownerMenu = false }) {
                            MenuLabel("Owner")

                            for (choice in listOf(forge.account) + forge.organisations) {
                                MenuItem(choice, onClick = { option.onChange(draft.copy(owner = choice)); ownerMenu = false }, checked = choice == owner)
                            }
                        }
                    }
                }
            }
        }
    }
}

private fun nodeNames(project: Project, nodes: Map<String, NodeSummary>): String = project.places.mapNotNull { nodes[it.nodeId]?.name }.joinToString(", ")

@Composable
fun ProjectMenu(projects: List<Project>, nodes: Map<String, NodeSummary>, selected: String?, label: String, onDismiss: () -> Unit, setUpFolder: String? = null, onSetUp: () -> Unit = {}, recent: List<Project> = emptyList(), noProjectLabel: String = "No project", showNodes: Boolean = true, onNewProject: (() -> Unit)? = null, newProject: NewProjectOption? = null, onSelect: (String?) -> Unit) {
    var query by remember { mutableStateOf("") }
    val matching = projects.filter { project ->
        listOf(project.name, project.path, nodeNames(project, nodes).takeIf { showNodes }.orEmpty()).any { it.contains(query.trim(), ignoreCase = true) }
    }

    val severalNodes = showNodes && projects.flatMap { held -> held.places.map { it.nodeId } }.distinct().size > 1
    val duplicateNames = projects.groupingBy { it.name }.eachCount().filterValues { it > 1 }.keys
    val hintFor = { option: Project -> if (showNodes) nodeNames(option, nodes).ifEmpty { null } else projectName(option.path).takeIf { option.name in duplicateNames } }
    val searching = query.isNotBlank()
    val recentShown = recent.filter { it in matching }.takeIf { !searching }.orEmpty()
    val rest = matching.filter { it !in recentShown }
    val touch = LocalTouch.current
    val searchFocus = remember { FocusRequester() }

    MenuPopup(onDismiss = onDismiss) {
        MenuLabel(label)

        if (newProject != null) {
            MenuItem("New project", onClick = newProject.onChoose, checked = newProject.chosen, hint = newProject.nodeName, leading = MyIcon.FolderPlus)

            if (newProject.chosen) {
                NewProjectForm(newProject)
            }

            Hairline(Modifier.padding(vertical = Tokens.Space.S1))
        }

        TextInput(query, { query = it }, label = "Search projects", placeholder = if (severalNodes) "Name, path or node" else "Name or path", showLabel = false, focusRequester = searchFocus)

        LaunchedEffect(Unit) {
            if (!touch) {
                searchFocus.requestFocus()
            }
        }

        val scroll = rememberLazyListState()

        LazyColumn(Modifier.heightIn(max = 320.dp).scrollbar(scroll), state = scroll) {
            if (recentShown.isNotEmpty()) {
                item { MenuLabel("Recent projects") }

                items(recentShown, key = { "recent-${it.id}" }) { option ->
                    MenuItem(option.name, onClick = { onSelect(option.id) }, checked = option.id == selected, hint = hintFor(option), leadingContent = { ProjectIcon(option) })
                }

                if (rest.isNotEmpty()) {
                    item { Hairline(Modifier.padding(vertical = Tokens.Space.S1)) }
                    item { MenuLabel("All projects") }
                }
            }

            items(rest, key = { it.id }) { option ->
                MenuItem(option.name, onClick = { onSelect(option.id) }, checked = option.id == selected, hint = hintFor(option)?.takeIf { !showNodes || severalNodes || recentShown.isNotEmpty() }, leadingContent = { ProjectIcon(option) })
            }

            if (matching.isEmpty()) {
                item { MenuLabel("No matching projects") }
            }
        }

        MenuItem(noProjectLabel, onClick = { onSelect(null) }, checked = selected == null && newProject?.chosen != true, leading = MyIcon.Folder)

        if (setUpFolder != null) {
            MenuItem("Set up ${projectName(setUpFolder)} as a project", onClick = onSetUp, leading = MyIcon.Plus)
        }

        if (onNewProject != null) {
            MenuItem("Set up a project", onClick = onNewProject, leading = MyIcon.Plus)
        }
    }
}
