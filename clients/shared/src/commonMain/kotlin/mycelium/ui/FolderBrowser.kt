package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.CancellationException
import mycelium.api.ApiException
import mycelium.api.Directories
import mycelium.api.NodeSummary
import mycelium.state.Store

@Composable
fun FolderBrowserDialog(store: Store, nodeId: String, nodeName: String, start: String, onChoose: (String) -> Unit, onDismiss: () -> Unit) {
    val colors = MyTheme.colors
    var listing by remember { mutableStateOf<Directories?>(null) }
    var requested by remember { mutableStateOf(start.trim()) }
    var failure by remember { mutableStateOf<String?>(null) }
    var fellBack by remember { mutableStateOf(false) }

    LaunchedEffect(nodeId, requested) {
        failure = null

        try {
            listing = store.directories(nodeId, requested)
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (problem: ApiException) {
            if (requested.isNotEmpty() && !fellBack) {
                fellBack = true
                requested = ""
            } else {
                failure = problem.message
            }
        } catch (_: Exception) {
            failure = "Could not reach the control plane. Check your connection and try again."
        }
    }

    Dialog(onDismiss) {
        Label("Choose a folder on $nodeName", Modifier.semantics { heading() }, Tokens.Type.Title)

        failure?.let { ErrorBanner(it, onDismiss = { failure = null }) }

        val shown = listing

        if (shown == null) {
            if (failure == null) {
                IndeterminateProgress("Loading folders")
            }
        } else {
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                shown.parent?.let { parent ->
                    MyButton({ requested = parent }, text = "Up", icon = MyIcon.ArrowUp, description = "Up to $parent", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
                }

                Label(shown.path, Modifier.weight(1f), Tokens.Type.CodeSm, colors.textMuted)
            }

            val scroll = rememberLazyListState()

            Column(Modifier.heightIn(max = 320.dp)) {
                LazyColumn(Modifier.scrollbar(scroll), state = scroll) {
                    items(shown.directories, key = { it.path }) { entry ->
                        MenuItem(
                            entry.name,
                            onClick = { requested = entry.path },
                            leading = MyIcon.Folder,
                            trailing = if (entry.repository) ({ Tag("Git") }) else null,
                        )
                    }

                    if (shown.directories.isEmpty()) {
                        item { MenuLabel("No folders here") }
                    }

                    if (shown.truncated) {
                        item { MenuLabel("Only the first folders are shown. Type the path to go further.") }
                    }
                }
            }
        }

        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End)) {
            MyButton(onDismiss, text = "Cancel", muted = true)
            MyButton({ listing?.let { onChoose(it.path) } }, text = "Choose this folder", variant = ButtonVariant.Outline)
        }
    }
}

@Composable
fun FolderField(store: Store, node: NodeSummary?, value: String, onValueChange: (String) -> Unit, label: String, placeholder: String, error: String? = null, onSubmit: (() -> Unit)? = null) {
    var browsing by remember { mutableStateOf(false) }

    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.Bottom) {
        TextInput(value, onValueChange, label = label, modifier = Modifier.weight(1f), placeholder = placeholder, mono = true, error = error, onSubmit = onSubmit)

        if (node != null) {
            MyButton({ browsing = true }, text = "Browse", icon = MyIcon.Folder, description = "Browse folders on ${node.name}", variant = ButtonVariant.Outline)
        }
    }

    if (browsing && node != null) {
        FolderBrowserDialog(store, node.id, node.name, value, onChoose = { onValueChange(it); browsing = false }, onDismiss = { browsing = false })
    }
}
