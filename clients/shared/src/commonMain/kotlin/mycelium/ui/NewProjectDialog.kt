package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import mycelium.api.CreateProjectRequest
import mycelium.api.Forge
import mycelium.api.Group
import mycelium.api.Project
import mycelium.api.Repository
import mycelium.state.AllGroups
import mycelium.state.AppState
import mycelium.state.NoGroup
import mycelium.state.Store

private const val RepositoryChoices = 6

private const val ForgeRepository = "A forge repository"

private const val NewRepository = "A new repository"

private const val NodeFolder = "A folder on a node"

private val ProjectSources = listOf(ForgeRepository, NewRepository, NodeFolder)

private fun forgeName(forge: Forge): String = "${forge.account} on ${forge.baseUrl.substringAfter("://")}"

@Composable
fun StepHeading(number: Int, title: String, filled: Boolean = true) {
    val colors = MyTheme.colors

    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
        Box(
            Modifier
                .size(Tokens.Size.StepCircle)
                .clip(CircleShape)
                .background(if (filled) colors.text else colors.bgPanel)
                .border(Tokens.Size.Hairline, if (filled) colors.text else colors.borderControl, CircleShape),
            contentAlignment = Alignment.Center,
        ) {
            Label(number.toString(), style = Tokens.Type.MetaStrong, color = if (filled) colors.bgPanel else colors.textMuted)
        }

        Label(title, Modifier.semantics { heading() }, Tokens.Type.UiStrong)
    }
}

@Composable
fun NewProjectDialog(store: Store, state: AppState, onDismiss: () -> Unit, onCreated: (Project) -> Unit, onExisting: (Project) -> Unit = onCreated) {
    val nodes = state.nodes.values.filter { it.online }.sortedBy { it.name }
    var source by remember { mutableStateOf(ProjectSources.first()) }
    var query by remember { mutableStateOf("") }
    var found by remember { mutableStateOf<List<Repository>>(emptyList()) }
    var repository by remember { mutableStateOf<Repository?>(null) }
    var forges by remember { mutableStateOf<List<Forge>>(emptyList()) }
    var forgeId by remember { mutableStateOf<String?>(null) }
    var created by remember { mutableStateOf("") }
    var folder by remember { mutableStateOf("") }
    var nodeId by remember { mutableStateOf<String?>(null) }
    var groups by remember { mutableStateOf<List<Group>>(emptyList()) }
    val groupFilter = LocalGroupFilter.current
    var groupId by remember { mutableStateOf<String?>(groupFilter.takeUnless { it == AllGroups || it == NoGroup || it.startsWith("all-except:") }) }
    val action = rememberAction()

    val cloning = source == ForgeRepository
    val existing = repository?.takeIf { cloning }?.let { picked -> state.projects.values.firstOrNull { it.forgeId == picked.forgeId && it.repository.equals(picked.fullName, ignoreCase = true) } }
    val node = nodes.firstOrNull { it.id == nodeId } ?: nodes.firstOrNull { existing?.checkoutOn(it.id) == null } ?: nodes.firstOrNull()
    val alreadyThere = existing != null && node != null && existing.checkoutOn(node.id) != null
    val creating = source == NewRepository
    val forge = forges.firstOrNull { it.id == forgeId } ?: forges.firstOrNull()
    val submitLabel = when {
        existing != null && alreadyThere -> "Open ${existing.name}"
        existing != null -> "Add a checkout on ${node?.name.orEmpty()}"
        cloning -> "Clone and open"
        creating -> "Create and open"
        else -> "Register and open"
    }
    val destination = when {
        cloning -> repository?.fullName
        creating -> created.trim().ifEmpty { null }
        else -> null
    }

    LaunchedEffect(query) {
        store.act { found = store.api.repositories(query = query).repositories.take(RepositoryChoices) }
    }

    LaunchedEffect(Unit) {
        store.act { groups = store.api.groups() }
    }

    LaunchedEffect(Unit) {
        store.act { forges = store.api.forges() }
    }

    fun submit() {
        if (action.running) {
            return
        }

        if (node == null) {
            action.failure = "Choose the node to set up on. Join a node from Nodes if none is online."
            return
        }

        if (cloning && repository == null) {
            action.failure = "Choose the repository to clone."
            return
        }

        if (existing != null && alreadyThere) {
            onExisting(existing)
            return
        }

        if (creating && forge == null) {
            action.failure = "Connect a forge in Settings, then create the repository."
            return
        }

        if (creating && !created.trim().contains('/')) {
            action.failure = "Enter the new repository as owner/name, for example acme-org/restore-tool."
            return
        }

        if (!cloning && !creating && folder.isBlank()) {
            action.failure = "Enter the folder on the node, for example work/app."
            return
        }

        val picked = repository
        val request = if (cloning && picked != null) {
            CreateProjectRequest(node.id, picked.forgeId, picked.fullName, groupId = groupId.takeIf { existing == null })
        } else if (creating && forge != null) {
            CreateProjectRequest(node.id, forge.id, created.trim(), newRepository = true, groupId = groupId)
        } else {
            CreateProjectRequest(node.id, path = folder.trim(), groupId = groupId)
        }

        action {
            val project = store.createProject(request)

            if (existing != null) {
                onExisting(project)
            } else {
                onCreated(project)
            }
        }
    }

    Dialog(onDismiss) {
        Label("New project", Modifier.semantics { heading() }, Tokens.Type.Title)

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            StepHeading(1, "Start from")
            FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                for (option in ProjectSources) {
                    MyButton(
                        { source = option; action.failure = null },
                        text = option,
                        variant = ButtonVariant.Outline,
                        size = ButtonSize.Sm,
                        muted = option != source,
                        selected = option == source,
                    )
                }
            }

            if (cloning) {
                TextInput(
                    repository?.fullName ?: query,
                    { repository = null; query = it },
                    label = "Search repositories",
                    placeholder = "Search repositories",
                    showLabel = false,
                    leadingIcon = MyIcon.Search,
                    trailing = { RefreshRepositoriesButton { store.act { found = store.api.repositories(query = query, refresh = true).repositories.take(RepositoryChoices) } } },
                )

                if (repository == null) {
                    for (option in found) {
                        MenuItem(option.fullName, onClick = { repository = option })
                    }
                }
            } else if (creating) {
                if (forges.size > 1) {
                    FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                        for (option in forges) {
                            MyButton(
                                { forgeId = option.id },
                                text = forgeName(option),
                                description = "Create on ${forgeName(option)}",
                                variant = ButtonVariant.Outline,
                                size = ButtonSize.Sm,
                                muted = option.id != forge?.id,
                                selected = option.id == forge?.id,
                            )
                        }
                    }
                }

                TextInput(created, { created = it }, label = "New repository", placeholder = "${forge?.organisations?.firstOrNull() ?: "owner"}/name", mono = true, onSubmit = ::submit)
            } else {
                TextInput(folder, { folder = it }, label = "Folder on the node", placeholder = "work/app", mono = true, onSubmit = ::submit)
            }
        }

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            StepHeading(2, "Set up on")

            FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                for (option in nodes) {
                    MyButton(
                        { nodeId = option.id },
                        text = option.name,
                        icon = nodeIcon(option),
                        iconColour = nodeColour(option),
                        description = if (existing?.checkoutOn(option.id) != null) "Set up on ${option.name}, already has a checkout" else "Set up on ${option.name}",
                        variant = ButtonVariant.Outline,
                        size = ButtonSize.Sm,
                        muted = option.id != node?.id,
                        selected = option.id == node?.id,
                        trailingIcon = MyIcon.Check.takeIf { existing?.checkoutOn(option.id) != null },
                    )
                }
            }

            if (existing != null) {
                Row(
                    Modifier.fillMaxWidth().background(MyTheme.colors.bgPanel, BlockShape).border(Tokens.Size.Hairline, MyTheme.colors.border, BlockShape).padding(Tokens.Space.S2),
                    horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                    verticalAlignment = Alignment.Top,
                ) {
                    Icon(MyIcon.Info, MyTheme.colors.textMuted, Tokens.Size.IconSm)
                    Label(
                        "${existing.repository} is already a project, checked out on ${existing.places.mapNotNull { state.nodes[it.nodeId]?.name }.joinToString(", ")}. Its group, icon and test command carry over.",
                        Modifier.weight(1f),
                        Tokens.Type.Meta,
                    )
                }
            }

            if (destination != null && existing == null) {
                Label("~/projects/$destination", style = Tokens.Type.Code, color = MyTheme.colors.textMuted)
            }
        }

        if (groups.isNotEmpty() && existing == null) {
            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                StepHeading(3, "Group", filled = false)

                FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                    for (group in groups) {
                        MyButton(
                            { groupId = if (groupId == group.id) null else group.id },
                            text = group.name,
                            description = "Group ${group.name}",
                            variant = ButtonVariant.Outline,
                            size = ButtonSize.Sm,
                            muted = group.id != groupId,
                            selected = group.id == groupId,
                        )
                    }
                }
            }
        }

        action.failure?.let { ErrorBanner(it, onDismiss = { action.failure = null }) }

        if (action.running) {
            IndeterminateProgress(if (cloning) "Cloning ${repository?.fullName.orEmpty()}" else if (creating) "Creating ${created.trim()}" else "Registering $folder")
        }

        FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            MyButton(onDismiss, text = "Cancel", muted = true)
            MyButton(::submit, text = submitLabel, variant = ButtonVariant.Outline)
        }
    }
}
