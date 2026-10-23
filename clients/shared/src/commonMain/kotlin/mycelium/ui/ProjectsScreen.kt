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
import mycelium.api.CreateProjectRequest
import mycelium.api.Forge
import mycelium.api.Project
import mycelium.api.Repositories
import kotlin.time.Instant
import mycelium.api.Repository
import mycelium.state.AppState
import mycelium.state.ago
import mycelium.state.lastActivity
import mycelium.state.Store
import mycelium.state.projectsOfRepository
import mycelium.state.registeredFolders

private fun forgeName(forge: Forge): String =
    if (forge.kind == "github") "GitHub" else forge.baseUrl.substringAfter("://").trimEnd('/')

private fun newConversationProject(projects: List<Project>, state: AppState): Project =
    projects.firstOrNull { state.nodes[it.nodeId]?.online == true } ?: projects.first()

private const val NotSetUpAnywhere = "none"

private sealed interface ProjectRow {
    val at: Instant?
}

private class RepositoryRow(val repository: Repository) : ProjectRow {
    override val at = repository.updatedAt?.let { runCatching { Instant.parse(it) }.getOrNull() }
}

private class FolderRow(val folder: Project, val updatedAt: String) : ProjectRow {
    override val at = runCatching { Instant.parse(updatedAt) }.getOrNull()
}

@Composable
fun ProjectsScreen(store: Store, state: AppState, onNewConversation: (String) -> Unit) {
    val colors = MyTheme.colors
    var forges by remember { mutableStateOf<List<Forge>?>(null) }
    var listing by remember { mutableStateOf<Repositories?>(null) }
    var query by remember { mutableStateOf("") }
    var forgeId by remember { mutableStateOf<String?>(null) }
    var organisation by remember { mutableStateOf<String?>(null) }
    var nodeFilter by remember { mutableStateOf<String?>(null) }
    var forgeMenu by remember { mutableStateOf(false) }
    var organisationMenu by remember { mutableStateOf(false) }
    var nodeMenu by remember { mutableStateOf(false) }
    var setUpMenu by remember { mutableStateOf<String?>(null) }
    var cloning by remember { mutableStateOf<Set<String>>(emptySet()) }
    var dismissed by remember { mutableStateOf<Set<String>>(emptySet()) }
    var creating by remember { mutableStateOf(false) }

    LaunchedEffect(Unit) {
        store.act { store.refreshProjects() }
    }

    LaunchedEffect(Unit) {
        store.act { forges = store.api.forges() }
    }

    LaunchedEffect(forgeId, organisation, query) {
        val asked = Triple(forgeId, organisation, query)

        store.act {
            val found = store.api.repositories(forgeId, organisation, query)

            if (asked == Triple(forgeId, organisation, query)) {
                listing = found
            }
        }
    }

    val connected = forges.orEmpty()
    val folders = if (forgeId == null && organisation == null) registeredFolders(state.projects.values).filter { it.name.contains(query.trim(), ignoreCase = true) && (nodeFilter == null || it.nodeId == nodeFilter) } else emptyList()
    val organisations = listing?.organisations.orEmpty().filter { forgeId == null || it.forgeId == forgeId }.map { it.name }.distinct()
    val repositories = listing?.repositories.orEmpty().filter { repository ->
        val setUp = projectsOfRepository(state.projects.values, repository)

        when (nodeFilter) {
            null -> true
            NotSetUpAnywhere -> setUp.isEmpty()
            else -> setUp.any { it.nodeId == nodeFilter }
        }
    }
    val rows = (repositories.map(::RepositoryRow) + folders.map { FolderRow(it, lastActivity(it, state.conversations.values)) }).sortedByDescending { it.at ?: Instant.DISTANT_PAST }
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
            TextInput(query, { query = it }, label = "Search repositories", placeholder = "Search repositories", showLabel = false, modifier = Modifier.width(Tokens.Size.FormColumn))

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
            ContentTable(listOf(ColumnSpec(flexible = true), ColumnSpec(max = 160.dp), ColumnSpec(max = 280.dp), ColumnSpec(max = 200.dp))) {
                if (forges != null && connected.isEmpty() && folders.isEmpty()) {
                    Label("No forge is connected. Connect GitHub or GitLab in Settings, Forges, to list repositories here.", Modifier.padding(Tokens.Space.S3), color = colors.textMuted)
                } else {
                    ContentRow(header = true) {
                        Label("Repository", style = Tokens.Type.MetaStrong, color = colors.textMuted)
                        Label("Forge", style = Tokens.Type.MetaStrong, color = colors.textMuted)
                        Label("Set up on", style = Tokens.Type.MetaStrong, color = colors.textMuted)
                        Box(Modifier)
                    }

                    Hairline()
                }

                for (row in rows) {
                    when (row) {
                        is RepositoryRow -> {
                            val repository = row.repository
                            val key = "${repository.forgeId}/${repository.fullName}"
                            val setUp = projectsOfRepository(state.projects.values, repository)
                            val forge = connected.firstOrNull { it.id == repository.forgeId }
                            val elsewhere = state.nodes.values.filter { node -> node.online && setUp.none { it.nodeId == node.id } }.sortedBy { it.name }

                            ContentRow {
                                Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalAlignment = Alignment.CenterVertically) {
                                    Label(repository.name, style = Tokens.Type.UiStrong, maxLines = 1)
                                    Label(repository.owner, Modifier.weight(1f, fill = false), color = colors.textFaint, maxLines = 1)
                                    repository.updatedAt?.let { Label("updated ${ago(it)}", style = Tokens.Type.Meta, color = colors.textFaint, maxLines = 1) }
                                }

                                Label(forge?.let(::forgeName).orEmpty(), color = colors.textMuted, maxLines = 1)

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
                                                MenuPopup(onDismiss = { setUpMenu = null }) {
                                                    MenuLabel("Set up on")

                                                    for (node in elsewhere) {
                                                        MenuItem(
                                                            node.name,
                                                            onClick = {
                                                                setUpMenu = null
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

                                Row(horizontalArrangement = Arrangement.End) {
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

                            Hairline()
                        }

                        is FolderRow -> {
                            val folder = row.folder
                            ContentRow {
                                Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalAlignment = Alignment.CenterVertically) {
                                    Label(folder.name, maxLines = 1)
                                    Label("updated ${ago(row.updatedAt)}", style = Tokens.Type.Meta, color = colors.textFaint, maxLines = 1)
                                }

                                Box(Modifier)

                                Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalAlignment = Alignment.CenterVertically) {
                                    NodeMarks(listOf(folder), state)
                                }

                                Row(horizontalArrangement = Arrangement.End) {
                                    MyButton(
                                        { onNewConversation(folder.id) },
                                        text = "New conversation",
                                        size = ButtonSize.Sm,
                                        variant = ButtonVariant.Outline,
                                        description = "New conversation in ${folder.name}",
                                    )
                                }
                            }

                            Hairline()
                        }
                    }
                }

                if (forges != null && connected.isNotEmpty() && listing != null && repositories.isEmpty() && folders.isEmpty()) {
                    Label("No repositories match.", Modifier.padding(Tokens.Space.S2), color = colors.textMuted)
                }
            }
        }
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
