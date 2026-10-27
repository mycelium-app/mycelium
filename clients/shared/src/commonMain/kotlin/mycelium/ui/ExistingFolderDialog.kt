package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import kotlinx.coroutines.launch
import mycelium.api.CreateProjectRequest
import mycelium.api.Repository
import mycelium.state.AppState
import mycelium.state.Store

@Composable
fun ExistingFolderDialog(store: Store, state: AppState, repository: Repository, onDismiss: () -> Unit) {
    val scope = rememberCoroutineScope()
    val nodes = state.nodes.values.filter { it.online }.sortedBy { it.name }
    var nodeId by remember { mutableStateOf<String?>(null) }
    var folder by remember { mutableStateOf("") }
    var running by remember { mutableStateOf(false) }
    var failure by remember { mutableStateOf<String?>(null) }
    val node = nodes.firstOrNull { it.id == nodeId } ?: nodes.firstOrNull()

    fun submit() {
        if (running) {
            return
        }

        if (node == null) {
            failure = "Choose the node that holds the folder. Join a node from Nodes if none is online."
            return
        }

        if (folder.isBlank()) {
            failure = "Enter the folder on the node, for example work/${repository.name}."
            return
        }

        failure = null
        running = true

        scope.launch {
            failure = attempt {
                store.createProject(CreateProjectRequest(node.id, repository.forgeId, repository.fullName, folder.trim()))
                onDismiss()
            }
            running = false
        }
    }

    Dialog(onDismiss) {
        Label("Use an existing folder for ${repository.fullName}", Modifier.semantics { heading() }, Tokens.Type.Title)

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            Label("Node", style = Tokens.Type.MetaStrong, color = MyTheme.colors.textMuted)

            FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                for (option in nodes) {
                    MyButton(
                        { nodeId = option.id },
                        text = option.name,
                        icon = nodeIcon(option),
                        description = "Folder on ${option.name}",
                        variant = ButtonVariant.Outline,
                        size = ButtonSize.Sm,
                        muted = option.id != node?.id,
                        selected = option.id == node?.id,
                    )
                }
            }

            FolderField(store, node, folder, { folder = it }, label = "Folder on the node", placeholder = "work/${repository.name}", onSubmit = ::submit)
        }

        failure?.let { ErrorBanner(it, onDismiss = { failure = null }) }

        if (running) {
            IndeterminateProgress("Checking $folder")
        }

        FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            MyButton(onDismiss, text = "Cancel", muted = true)
            MyButton(::submit, text = "Use folder", variant = ButtonVariant.Outline)
        }
    }
}
