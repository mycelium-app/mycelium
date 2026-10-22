package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
import mycelium.api.ApiException
import mycelium.api.CreateProjectRequest
import mycelium.api.Project
import mycelium.api.Repository
import mycelium.state.AppState
import mycelium.state.Store

private const val RepositoryChoices = 6

private val ProjectSources = listOf("A forge repository", "A folder on a node")

@Composable
fun NewProjectDialog(store: Store, state: AppState, onDismiss: () -> Unit, onCreated: (Project) -> Unit) {
    val scope = rememberCoroutineScope()
    val nodes = state.nodes.values.filter { it.online }.sortedBy { it.name }
    var source by remember { mutableStateOf(ProjectSources.first()) }
    var query by remember { mutableStateOf("") }
    var found by remember { mutableStateOf<List<Repository>>(emptyList()) }
    var repository by remember { mutableStateOf<Repository?>(null) }
    var folder by remember { mutableStateOf("") }
    var nodeId by remember { mutableStateOf<String?>(null) }
    var running by remember { mutableStateOf(false) }
    var failure by remember { mutableStateOf<String?>(null) }

    val node = nodes.firstOrNull { it.id == nodeId } ?: nodes.firstOrNull()
    val cloning = source == ProjectSources.first()
    val action = if (cloning) "Clone and open" else "Register and open"

    LaunchedEffect(query) {
        store.act { found = store.api.repositories(query = query).repositories.take(RepositoryChoices) }
    }

    fun submit() {
        if (running) {
            return
        }

        if (node == null) {
            failure = "Choose the node to set up on. Join a node from Nodes if none is online."
            return
        }

        if (cloning && repository == null) {
            failure = "Choose the repository to clone."
            return
        }

        if (!cloning && folder.isBlank()) {
            failure = "Enter the folder on the node, for example work/app."
            return
        }

        val picked = repository
        val request = if (cloning && picked != null) {
            CreateProjectRequest(node.id, picked.forgeId, picked.fullName)
        } else {
            CreateProjectRequest(node.id, path = folder.trim())
        }

        failure = null
        running = true

        scope.launch {
            try {
                onCreated(store.createProject(request))
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (problem: ApiException) {
                failure = problem.message
            } catch (_: Exception) {
                failure = "Could not reach the control plane. Check your connection and try again."
            } finally {
                running = false
            }
        }
    }

    Dialog(onDismiss) {
        Label("New project", Modifier.semantics { heading() }, Tokens.Type.Title)

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            Label("Start from", style = Tokens.Type.MetaStrong, color = MyTheme.colors.textMuted)
            Segmented(ProjectSources, source, { source = it; failure = null }, groupLabel = "Start from")

            if (cloning) {
                TextInput(query, { query = it }, label = "Search repositories", placeholder = "Search repositories", showLabel = false)

                for (option in found) {
                    MenuItem(option.fullName, onClick = { repository = option }, checked = option.fullName == repository?.fullName && option.forgeId == repository?.forgeId)
                }
            } else {
                TextInput(folder, { folder = it }, label = "Folder on the node", placeholder = "work/app", mono = true, onSubmit = ::submit)
            }
        }

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            Label("Set up on", style = Tokens.Type.MetaStrong, color = MyTheme.colors.textMuted)

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
        }

        failure?.let { ErrorBanner(it, onDismiss = { failure = null }) }

        if (running) {
            IndeterminateProgress(if (cloning) "Cloning ${repository?.fullName.orEmpty()}" else "Registering $folder")
        }

        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End)) {
            MyButton(onDismiss, text = "Cancel", muted = true)
            MyButton(::submit, text = action, variant = ButtonVariant.Outline)
        }
    }
}
