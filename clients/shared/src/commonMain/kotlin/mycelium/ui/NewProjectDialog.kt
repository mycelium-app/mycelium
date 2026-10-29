package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
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
import mycelium.state.AppState
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
fun NewProjectDialog(store: Store, state: AppState, onDismiss: () -> Unit, onCreated: (Project) -> Unit) {
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
    var groupId by remember { mutableStateOf<String?>(null) }
    val action = rememberAction()

    val node = nodes.firstOrNull { it.id == nodeId } ?: nodes.firstOrNull()
    val cloning = source == ForgeRepository
    val creating = source == NewRepository
    val forge = forges.firstOrNull { it.id == forgeId } ?: forges.firstOrNull()
    val submitLabel = when {
        cloning -> "Clone and open"
        creating -> "Create and open"
        else -> "Register and open"
    }
    val destination = when {
        cloning -> repository?.fullName
        creating -> created.trim().ifEmpty { null }
        else -> folder.trim().ifEmpty { null }
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
            CreateProjectRequest(node.id, picked.forgeId, picked.fullName)
        } else if (creating && forge != null) {
            CreateProjectRequest(node.id, forge.id, created.trim(), newRepository = true)
        } else {
            CreateProjectRequest(node.id, path = folder.trim())
        }

        action {
            val project = store.createProject(request)
            val chosen = groupId

            onCreated(if (chosen == null) project else store.setProjectGroup(project.id, chosen))
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
                TextInput(repository?.fullName ?: query, { repository = null; query = it }, label = "Search repositories", placeholder = "Search repositories", showLabel = false, leadingIcon = MyIcon.Search)

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
                        description = "Set up on ${option.name}",
                        variant = ButtonVariant.Outline,
                        size = ButtonSize.Sm,
                        muted = option.id != node?.id,
                        selected = option.id == node?.id,
                    )
                }
            }

            if (destination != null) {
                Label("~/projects/$destination", style = Tokens.Type.Code, color = MyTheme.colors.textMuted)
            }
        }

        if (groups.isNotEmpty()) {
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
