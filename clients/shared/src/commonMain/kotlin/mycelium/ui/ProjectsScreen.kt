package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.unit.dp
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import mycelium.state.ColumnAlign
import mycelium.api.CreateProjectRequest
import mycelium.api.Forge
import mycelium.api.Project
import mycelium.api.Repositories
import kotlin.time.Instant
import mycelium.api.Repository
import mycelium.state.AppState
import mycelium.state.lastActivity
import mycelium.state.Store
import mycelium.state.inGroup
import mycelium.state.projectsOfRepository
import mycelium.state.registeredFolders
import mycelium.state.setUpRepositories

private fun forgeName(forge: Forge): String =
    if (forge.kind == "github") "GitHub" else forge.baseUrl.substringAfter("://").trimEnd('/')

private fun newConversationProject(projects: List<Project>, state: AppState): Project =
    projects.firstOrNull { state.nodes[it.nodeId]?.online == true } ?: projects.first()

private const val NotSetUpAnywhere = "none"

private sealed interface ProjectRow {
    val at: Instant?
}

private class RepositoryRow(val repository: Repository, override val at: Instant?) : ProjectRow

private class FolderRow(val folder: Project, val updatedAt: String) : ProjectRow {
    override val at = runCatching { Instant.parse(updatedAt) }.getOrNull()
}

private const val RepositoriesList = "repositories"

@Composable
fun ProjectsScreen(store: Store, state: AppState, onNewConversation: (String) -> Unit) {
    val colors = MyTheme.colors
    var filtered by remember { mutableStateOf<Repositories?>(null) }
    var query by remember { mutableStateOf("") }
    var forgeId by remember { mutableStateOf<String?>(null) }
    var organisation by remember { mutableStateOf<String?>(null) }
    var nodeFilter by remember { mutableStateOf<String?>(null) }
    var forgeMenu by remember { mutableStateOf(false) }
    var organisationMenu by remember { mutableStateOf(false) }
    var nodeMenu by remember { mutableStateOf(false) }
    var setUpMenu by remember { mutableStateOf<String?>(null) }
    var cloneChoice by remember { mutableStateOf(false) }
    var existingFolder by remember { mutableStateOf<Repository?>(null) }
    var cloning by remember { mutableStateOf<Set<String>>(emptySet()) }
    var dismissed by remember { mutableStateOf<Set<String>>(emptySet()) }
    var creating by remember { mutableStateOf(false) }

    LaunchedEffect(Unit) {
        store.load { store.refreshProjects() }
        store.load { store.refreshGroups() }
    }

    val loadFailures by store.loadFailures.collectAsState()
    val loadRepositories = { store.load(RepositoriesList) { store.refreshRepositories() } }

    LaunchedEffect(Unit) {
        loadRepositories()
    }

    val filtering = forgeId != null || organisation != null || query.isNotBlank()

    LaunchedEffect(forgeId, organisation, query) {
        if (!filtering) {
            return@LaunchedEffect
        }

        val asked = Triple(forgeId, organisation, query)

        store.act {
            val found = store.api.repositories(forgeId, organisation, query)

            if (asked == Triple(forgeId, organisation, query)) {
                filtered = found
            }
        }
    }

    val forges = state.forges
    val groupFilter = LocalGroupFilter.current
    val visibleProjects = state.projects.values.filter { inGroup(it.groupId, groupFilter) }
    val listing = if (filtering) filtered ?: state.repositoryListing else state.repositoryListing
    val connected = forges.orEmpty()
    val folders = if (forgeId == null && organisation == null) registeredFolders(visibleProjects).filter { it.name.contains(query.trim(), ignoreCase = true) && (nodeFilter == null || it.nodeId == nodeFilter) } else emptyList()
    val organisations = listing?.organisations.orEmpty().filter { forgeId == null || it.forgeId == forgeId }.map { it.name }.distinct()
    val setUpListed = setUpRepositories(visibleProjects, listing?.repositories.orEmpty()).filter { repository ->
        val matchesSearch = (forgeId == null || repository.forgeId == forgeId) && (organisation == null || repository.owner == organisation) && repository.fullName.contains(query.trim(), ignoreCase = true)

        matchesSearch && (nodeFilter == null || projectsOfRepository(visibleProjects, repository).any { it.nodeId == nodeFilter })
    }
    val notSetUp = listing?.repositories.orEmpty().filter { repository ->
        (nodeFilter == null || nodeFilter == NotSetUpAnywhere) && projectsOfRepository(visibleProjects, repository).isEmpty()
    }
    val repositories = setUpListed + notSetUp
    val initialised = setUpListed.map { repository ->
        val active = projectsOfRepository(visibleProjects, repository).mapNotNull { runCatching { Instant.parse(lastActivity(it, state.conversations.values)) }.getOrNull() }.maxOrNull()

        RepositoryRow(repository, active)
    } + folders.map { FolderRow(it, lastActivity(it, state.conversations.values)) }
    val rows = initialised.sortedByDescending { it.at ?: Instant.DISTANT_PAST } + notSetUp.map { RepositoryRow(it, it.updatedAt?.let { at -> runCatching { Instant.parse(at) }.getOrNull() }) }.sortedByDescending { it.at ?: Instant.DISTANT_PAST }
    val chosenNode = state.nodes[nodeFilter]

    Column(Modifier.fillMaxSize()) {
        Bar {
            BarTitle("Projects")
            Box(Modifier.weight(1f))
            MyButton({ creating = true }, text = "New project", icon = MyIcon.Plus, variant = ButtonVariant.Outline)
        }

        for (failure in listing?.failures.orEmpty().filter { it.forgeId !in dismissed }) {
            ErrorBanner(failure.message, onDismiss = { dismissed = dismissed + failure.forgeId })
        }

        FlowRow(
            Modifier.fillMaxWidth().padding(Tokens.Space.S3),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
            verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
            itemVerticalAlignment = Alignment.CenterVertically,
        ) {
            ListSearch(query, { query = it }, "repositories")

            Box {
                val chosen = connected.firstOrNull { it.id == forgeId }

                MyButton(
                    { forgeMenu = true },
                    text = chosen?.let(::forgeName) ?: "All forges",
                    description = "Forge: ${chosen?.let(::forgeName) ?: "All forges"}",
                    variant = ButtonVariant.Outline,
                    size = ButtonSize.Sm,
                    muted = chosen == null,
                    trailingIcon = MyIcon.ChevronDown,
                )

                if (forgeMenu) {
                    MenuPopup(onDismiss = { forgeMenu = false }) {
                        MenuLabel("Forge")
                        MenuItem("All forges", onClick = { forgeId = null; organisation = null; forgeMenu = false }, checked = forgeId == null)

                        for (forge in connected) {
                            MenuItem(forgeName(forge), onClick = { forgeId = forge.id; organisation = null; forgeMenu = false }, checked = forge.id == forgeId, hint = forge.account)
                        }
                    }
                }
            }

            Box {
                MyButton(
                    { organisationMenu = true },
                    text = organisation ?: "All organisations",
                    description = "Organisation: ${organisation ?: "All organisations"}",
                    variant = ButtonVariant.Outline,
                    size = ButtonSize.Sm,
                    muted = organisation == null,
                    trailingIcon = MyIcon.ChevronDown,
                )

                if (organisationMenu) {
                    MenuPopup(onDismiss = { organisationMenu = false }) {
                        MenuLabel("Organisation")
                        MenuItem("All organisations", onClick = { organisation = null; organisationMenu = false }, checked = organisation == null)

                        for (name in organisations) {
                            MenuItem(name, onClick = { organisation = name; organisationMenu = false }, checked = name == organisation)
                        }
                    }
                }
            }

            Box {
                val nodeLabel = chosenNode?.name ?: if (nodeFilter == NotSetUpAnywhere) "Not set up anywhere" else "All nodes"

                MyButton(
                    { nodeMenu = true },
                    text = nodeLabel,
                    description = "Node: $nodeLabel",
                    variant = ButtonVariant.Outline,
                    size = ButtonSize.Sm,
                    muted = nodeFilter == null,
                    trailingIcon = MyIcon.ChevronDown,
                )

                if (nodeMenu) {
                    MenuPopup(onDismiss = { nodeMenu = false }) {
                        MenuLabel("Set up on")
                        MenuItem("All nodes", onClick = { nodeFilter = null; nodeMenu = false }, checked = nodeFilter == null)

                        for (node in state.nodes.values.sortedBy { it.name }) {
                            MenuItem(node.name, onClick = { nodeFilter = node.id; nodeMenu = false }, checked = node.id == nodeFilter, leading = nodeIcon(node))
                        }

                        MenuItem("Not set up anywhere", onClick = { nodeFilter = NotSetUpAnywhere; nodeMenu = false }, checked = nodeFilter == NotSetUpAnywhere)
                    }
                }
            }
        }

        Hairline()

        Column(Modifier.weight(1f).fillMaxWidth().verticalScrollWithBar().padding(Tokens.Space.S4)) {
            val repositoriesFailed = forges == null && RepositoriesList in loadFailures

            if (repositoriesFailed && folders.isEmpty()) {
                LoadFailedState("repositories", loadRepositories, Modifier.fillMaxWidth().padding(vertical = Tokens.Space.S4))
            } else if (forges != null && connected.isEmpty() && folders.isEmpty()) {
                Label("No forge is connected. Connect GitHub or GitLab in Settings, Forges, to list repositories here.", Modifier.padding(Tokens.Space.S3), color = colors.textMuted)
            } else {
                if (repositoriesFailed) {
                    LoadFailedState("repositories", loadRepositories, Modifier.fillMaxWidth().padding(vertical = Tokens.Space.S2))
                }

                Table(
                    listOf(
                        Col("Repository", 200.dp, weight = 1f),
                        Col("Forge", 72.dp, max = 160.dp, priority = 1),
                        Col("Set up on", 120.dp, max = 280.dp, overflow = Overflow.Wrap, priority = 1),
                        Col("", 190.dp, preferred = 190.dp, align = ColumnAlign.End),
                    ),
                    rows.size,
                    { index, column ->
                        when (val row = rows[index]) {
                            is RepositoryRow -> {
                                val repository = row.repository
                                val key = "${repository.forgeId}/${repository.fullName}"
                                val setUp = projectsOfRepository(visibleProjects, repository)
                                val forge = connected.firstOrNull { it.id == repository.forgeId }
                                val elsewhere = state.nodes.values.filter { node -> node.online && setUp.none { it.nodeId == node.id } }.sortedBy { it.name }

                                when (column) {
                                    0 -> TableCell.Slot("${repository.name} ${repository.owner} updated 12 minutes ago", Tokens.Size.IconMd + Tokens.Space.S1_5 * 3) {
                                        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalAlignment = Alignment.CenterVertically) {
                                            ProjectIcon(setUp.firstOrNull(), Tokens.Size.IconMd)
                                            Label(repository.name, style = Tokens.Type.UiStrong, maxLines = 1)
                                            Label(repository.owner, Modifier.weight(1f, fill = false), color = colors.textFaint, maxLines = 1)
                                            repository.updatedAt?.let { Label("updated ${timestamp(it, ago = true)}", style = Tokens.Type.Meta, color = colors.textFaint, maxLines = 1) }
                                        }
                                    }

                                    1 -> TableCell.Text(forge?.let(::forgeName).orEmpty(), color = colors.textMuted)
                                    2 -> TableCell.Slot(setUp.mapNotNull { state.nodes[it.nodeId]?.name }.joinToString(" "), Tokens.Size.IconMd + Tokens.Size.IconMd) {
                                        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalAlignment = Alignment.CenterVertically) {

                                            NodeMarks(setUp, state)

                                            if (key in cloning) {
                                                Box(Modifier.width(Tokens.Size.NumberField)) {
                                                    IndeterminateProgress("Cloning ${repository.fullName}")
                                                }
                                            } else if (elsewhere.isNotEmpty()) {
                                                Box {
                                                    MyButton(
                                                        { setUpMenu = key },
                                                        icon = MyIcon.Plus,
                                                        description = if (setUp.isEmpty()) "Set up ${repository.fullName} on a node" else "Set up ${repository.fullName} on another node",
                                                        tooltip = if (setUp.isEmpty()) "Set up on a node" else "Set up on another node",
                                                        size = ButtonSize.Sm,
                                                        muted = true,
                                                    )

                                                    if (setUpMenu == key) {
                                                        MenuPopup(onDismiss = { setUpMenu = null; cloneChoice = false }) {
                                                            if (!cloneChoice) {
                                                                MenuLabel("Set up on")
                                                                MenuItem("Clone onto a node", onClick = { cloneChoice = true })
                                                                MenuItem("Use an existing folder", onClick = { setUpMenu = null; existingFolder = repository })
                                                            } else {
                                                                MenuLabel("Clone onto")

                                                                for (node in elsewhere) {
                                                                    MenuItem(
                                                                        node.name,
                                                                        onClick = {
                                                                            setUpMenu = null
                                                                            cloneChoice = false
                                                                            cloning = cloning + key
                                                                            store.act {
                                                                                try {
                                                                                    store.createProject(CreateProjectRequest(node.id, repository.forgeId, repository.fullName))
                                                                                } finally {
                                                                                    cloning = cloning - key
                                                                                }
                                                                            }
                                                                        },
                                                                        leading = nodeIcon(node),
                                                                    )
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    else -> TableCell.Slot {
                                        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5, Alignment.End), verticalAlignment = Alignment.CenterVertically) {
                                            if (setUp.isNotEmpty()) {
                                                ProjectGroupMenu(store, state, setUp, repository.fullName)
                                            }

                                            if (setUp.isNotEmpty()) {
                                                MyButton(
                                                    { onNewConversation(newConversationProject(setUp, state).id) },
                                                    text = "New conversation",
                                                    size = ButtonSize.Sm,
                                                    variant = ButtonVariant.Outline,
                                                    description = "New conversation in ${repository.fullName}",
                                                )
                                            }
                                        }
                                    }
                                }
                            }

                            is FolderRow -> {
                                val folder = row.folder

                                when (column) {
                                    0 -> TableCell.Slot("${folder.name} updated 12 minutes ago", Tokens.Size.IconMd + Tokens.Space.S1_5 * 2) {
                                        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalAlignment = Alignment.CenterVertically) {
                                            ProjectIcon(folder, Tokens.Size.IconMd)
                                            Label(folder.name, maxLines = 1)
                                            Label("updated ${timestamp(row.updatedAt, ago = true)}", style = Tokens.Type.Meta, color = colors.textFaint, maxLines = 1)
                                        }
                                    }

                                    1 -> TableCell.Text("")
                                    2 -> TableCell.Slot(state.nodes[folder.nodeId]?.name.orEmpty(), Tokens.Size.IconMd) {
                                        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalAlignment = Alignment.CenterVertically) {
                                            NodeMarks(listOf(folder), state)
                                        }
                                    }

                                    else -> TableCell.Slot {
                                        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5, Alignment.End), verticalAlignment = Alignment.CenterVertically) {
                                            ProjectGroupMenu(store, state, listOf(folder), folder.name)

                                            MyButton(
                                                { onNewConversation(folder.id) },
                                                text = "New conversation",
                                                size = ButtonSize.Sm,
                                                variant = ButtonVariant.Outline,
                                                description = "New conversation in ${folder.name}",
                                            )
                                        }
                                    }
                                }
                            }
                        }
                    },
                    lazy = false,
                )
            }

            if (forges != null && connected.isNotEmpty() && listing != null && repositories.isEmpty() && folders.isEmpty()) {
                Label("No repositories match.", Modifier.padding(Tokens.Space.S2), color = colors.textMuted)
            }
        }
    }

    existingFolder?.let { chosen ->
        ExistingFolderDialog(store, state, chosen, onDismiss = { existingFolder = null })
    }

    if (creating) {
        NewProjectDialog(store, state, onDismiss = { creating = false }, onCreated = { creating = false; onNewConversation(it.id) })
    }
}

@Composable
private fun NodeMarks(projects: List<Project>, state: AppState) {
    for (project in projects) {
        val node = state.nodes[project.nodeId] ?: continue

        Row(Modifier.semantics { contentDescription = "Set up on ${node.name}" }, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
            Icon(nodeIcon(node), nodeColour(node), Tokens.Size.IconMd)
            Label(node.name, maxLines = 1)
        }
    }
}
