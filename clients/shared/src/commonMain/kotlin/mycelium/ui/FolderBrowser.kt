package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import mycelium.api.Directories
import mycelium.api.NodeSummary
import mycelium.state.Store
import mycelium.state.randomFolderName
import kotlinx.coroutines.launch

@Composable
fun FolderBrowserDialog(store: Store, nodeId: String, nodeName: String, start: String, onChoose: (String) -> Unit, onDismiss: () -> Unit) {
    val colors = MyTheme.colors
    var listing by remember { mutableStateOf<Directories?>(null) }
    var requested by remember { mutableStateOf(start.trim()) }
    var failure by remember { mutableStateOf<String?>(null) }
    var fellBack by remember { mutableStateOf(false) }
    var attempts by remember { mutableStateOf(0) }
    var naming by remember { mutableStateOf<String?>(null) }
    val namingFocus = remember { FocusRequester() }
    val scope = rememberCoroutineScope()

    LaunchedEffect(nodeId, requested, attempts) {
        failure = null

        val problem = attempt { listing = store.directories(nodeId, requested) }

        if (problem != null && requested.isNotEmpty() && !fellBack) {
            fellBack = true
            requested = ""
        } else {
            failure = problem
        }
    }

    Dialog(onDismiss) {
        Label("Choose a folder on $nodeName", Modifier.semantics { heading() }, Tokens.Type.Title)

        failure?.let { ErrorBanner(it, onDismiss = { failure = null }, actions = { MyButton({ attempts += 1 }, text = "Retry", description = "Retry", size = ButtonSize.Sm, variant = ButtonVariant.Outline) }) }

        val shown = listing

        if (shown == null) {
            if (failure == null) {
                IndeterminateProgress("Loading folders")
            }
        } else {
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                Label(shown.path, Modifier.weight(1f), Tokens.Type.CodeSm, colors.textMuted)

                MyButton({ naming = randomFolderName() }, text = "New folder", icon = MyIcon.FolderPlus, variant = ButtonVariant.Outline, size = ButtonSize.Sm, selected = naming != null)

                shown.parent?.let { parent ->
                    MyButton({ requested = parent }, text = "Up", icon = MyIcon.ArrowUp, description = "Up to $parent", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
                }
            }

            val scroll = rememberLazyListState()

            Column(Modifier.heightIn(max = 320.dp)) {
                LazyColumn(Modifier.scrollbar(scroll), state = scroll) {
                    naming?.let { name ->
                        item {
                            Row(Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                                Icon(MyIcon.FolderPlus, colors.textMuted)

                                TextInput(
                                    name,
                                    { naming = it },
                                    label = "New folder name",
                                    modifier = Modifier
                                        .weight(1f)
                                        .onPreviewKeyEvent { event ->
                                            if (event.type == KeyEventType.KeyDown && event.key == Key.Escape) {
                                                naming = null
                                                true
                                            } else {
                                                false
                                            }
                                        },
                                    showLabel = false,
                                    mono = true,
                                    focusRequester = namingFocus,
                                    onSubmit = {
                                        scope.launch {
                                            failure = attempt {
                                                val made = store.createDirectory(nodeId, shown.path, name)

                                                naming = null
                                                requested = made.path
                                            }
                                        }
                                    },
                                )
                            }

                            LaunchedEffect(Unit) { namingFocus.requestFocus() }
                        }
                    }

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

        FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            MyButton(onDismiss, text = "Cancel", variant = ButtonVariant.Outline)
            MyButton({ listing?.let { onChoose(it.path) } }, text = "Choose this folder", variant = ButtonVariant.Primary)
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
