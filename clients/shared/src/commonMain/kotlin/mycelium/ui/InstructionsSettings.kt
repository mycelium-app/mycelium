package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalWindowInfo
import androidx.compose.ui.unit.dp
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
import mycelium.api.ApiException
import mycelium.api.Extensions
import mycelium.api.Forge
import mycelium.api.InstructionLayer
import mycelium.api.Instructions
import mycelium.api.SetInstructionSourceRequest
import mycelium.state.Store
import mycelium.state.heldVersion
import mycelium.state.layerFile
import mycelium.state.olderSessions
import mycelium.state.shortVersion

private const val GlobalLayer = "global"

private fun forgeLabel(forge: Forge): String = "${if (forge.kind == "github") "GitHub" else "GitLab"} ${forge.account}"

@Composable
private fun ExtensionSwitch(name: String, hint: String, on: Boolean, onToggle: () -> Unit) {
    Row(Modifier.fillMaxWidth().heightIn(min = rowHeight()).padding(horizontal = Tokens.Space.S2), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
        Checkbox(on, name, onToggle, checkedState = "On", uncheckedState = "Off")

        Column(Modifier.weight(1f)) {
            Label(name)
            Label(hint, style = Tokens.Type.Meta, color = MyTheme.colors.textMuted)
        }
    }
}

@Composable
internal fun InstructionsEditor(layer: InstructionLayer, file: String, saveLabel: String, onSave: (String) -> Unit, actions: @Composable () -> Unit = {}, editLabel: String = "Edit ${layer.name} instructions") {
    val colors = MyTheme.colors
    var draft by remember { mutableStateOf(layer.text) }
    val viewportHeight = with(LocalDensity.current) { LocalWindowInfo.current.containerSize.height.toDp() }
    val editorCap = maxOf(Tokens.Size.InstructionsEditorCap, viewportHeight * 0.6f)

    LaunchedEffect(layer.layer, layer.version?.id) {
        draft = layer.text
    }

    Column(Modifier.fillMaxWidth().border(Tokens.Size.Hairline, colors.border, ControlShape)) {
        Row(
            Modifier.fillMaxWidth().background(colors.bgPanel).padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Label(file, Modifier.weight(1f), Tokens.Type.Code, maxLines = 1)
            Label(layer.version?.let { "${shortVersion(it.id)} · ${timestamp(it.createdAt)}" } ?: "No version yet", style = Tokens.Type.Meta, color = colors.textFaint, maxLines = 1)
        }

        Box(Modifier.fillMaxWidth().heightIn(max = editorCap).verticalScrollWithBar()) {
            BasicTextField(
                value = draft,
                onValueChange = { draft = it },
                textStyle = Tokens.Type.Code.themed().copy(color = colors.text),
                cursorBrush = SolidColor(colors.text),
                modifier = Modifier
                    .fillMaxWidth()
                    .heightIn(min = Tokens.Size.InstructionsEditor)
                    .padding(Tokens.Space.S3)
                    .semantics { contentDescription = editLabel },
            )
        }
    }

    FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2), itemVerticalAlignment = Alignment.CenterVertically) {
        MyButton({ onSave(draft) }, text = saveLabel, variant = ButtonVariant.Outline)
        actions()
    }
}

@Composable
fun InstructionsSettings(store: Store, onOpenGroup: ((String) -> Unit)? = null) {
    val colors = MyTheme.colors
    val state by store.state.collectAsState()
    var document by remember { mutableStateOf<Instructions?>(null) }
    var forges by remember { mutableStateOf<List<Forge>>(emptyList()) }
    var extensions by remember { mutableStateOf<Extensions?>(null) }
    var failure by remember { mutableStateOf<String?>(null) }
    var choosingSource by remember { mutableStateOf(false) }
    var sourceForge by remember { mutableStateOf<String?>(null) }
    var sourceRepository by remember { mutableStateOf("") }
    var sourcePath by remember { mutableStateOf("") }
    var forgeMenu by remember { mutableStateOf(false) }

    val layers = document?.layers.orEmpty()
    val layer = layers.firstOrNull { it.layer == GlobalLayer }
    val source = document?.source

    val attempt = rememberAttempt { failure = it }

    LaunchedEffect(Unit) {
        store.act {
            document = store.api.instructions()
            forges = store.api.forges()
            extensions = store.api.extensions()
        }
    }

    Column(Modifier.widthIn(max = Tokens.Size.Page), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4)) {
        Label("Instructions", Modifier.semantics { heading() }, Tokens.Type.UiStrong)

        failure?.let { ErrorBanner(it, onDismiss = { failure = null }) }

        if (layer != null) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Label("Global instructions", Modifier.weight(1f).semantics { heading() }, Tokens.Type.Title)

                RowMenu("${layer.name} instructions") { close ->
                    MenuItem(if (source != null) "Change repository" else "Connect a repository", onClick = {
                        close()
                        choosingSource = !choosingSource
                        sourceForge = source?.forgeId ?: forges.firstOrNull()?.id
                        sourceRepository = source?.repository.orEmpty()
                        sourcePath = source?.path.orEmpty()
                    })
                }
            }

            if (choosingSource) {
                Column(Modifier.widthIn(max = Tokens.Size.FormColumn), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
                    if (forges.isEmpty()) {
                        Label("Connect GitHub or GitLab in Settings, Forges, first.", color = colors.textMuted)
                    } else {
                        Box {
                            val chosen = forges.firstOrNull { it.id == sourceForge }

                            MyButton(
                                { forgeMenu = true },
                                text = chosen?.let(::forgeLabel) ?: "Choose a forge",
                                description = "Forge: ${chosen?.let(::forgeLabel) ?: "none chosen"}",
                                variant = ButtonVariant.Outline,
                                size = ButtonSize.Sm,
                                trailingIcon = MyIcon.ChevronDown,
                            )

                            if (forgeMenu) {
                                MenuPopup(onDismiss = { forgeMenu = false }) {
                                    MenuLabel("Forge")

                                    for (forge in forges) {
                                        MenuItem(forgeLabel(forge), onClick = { sourceForge = forge.id; forgeMenu = false }, checked = forge.id == sourceForge)
                                    }
                                }
                            }
                        }

                        TextInput(sourceRepository, { sourceRepository = it }, label = "Repository", placeholder = "owner/name", mono = true)
                        TextInput(sourcePath, { sourcePath = it }, label = "Path of the global file", placeholder = "AGENTS.md", mono = true)

                        FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                            MyButton(
                                {
                                    val forgeId = sourceForge

                                    if (forgeId == null) {
                                        failure = "Choose the forge the repository is on."
                                    } else {
                                        attempt {
                                            document = store.api.setInstructionSource(SetInstructionSourceRequest(forgeId, sourceRepository, sourcePath.ifBlank { null }))
                                            document = store.api.pullInstructions()
                                            choosingSource = false
                                        }
                                    }
                                },
                                text = "Connect and pull",
                                variant = ButtonVariant.Outline,
                            )

                            if (source != null) {
                                MyButton({ attempt { store.api.clearInstructionSource(); document = store.api.instructions(); choosingSource = false } }, text = "Disconnect", muted = true)
                            }

                            MyButton({ choosingSource = false }, text = "Cancel", muted = true)
                        }
                    }
                }
            }
        }

        if (layer != null) {
            InstructionsEditor(
                layer = layer,
                file = layerFile(layer, source?.repository),
                saveLabel = if (source != null) "Save and commit" else "Save",
                onSave = { text -> attempt { store.api.saveInstructions(layer.layer, text); document = store.api.instructions() } },
                actions = {
                    if (source != null) {
                        MyButton({ attempt { document = store.api.pullInstructions() } }, text = "Pull", muted = true)
                    }
                },
            )
        }

        InstructionProposals(store, onOpenGroup)

        extensions?.let { held ->
            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                Label("Mycelium extensions", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
                Label("Built-in additions to the instructions of conversations. A session picks up a change before its next message.", style = Tokens.Type.Meta, color = colors.textMuted)

                ExtensionSwitch("Generated UI", "Teaches agents to answer with tables, charts and buttons you can press. Added to Claude Code and Codex sessions.", held.generatedUi) {
                    attempt { extensions = store.api.saveExtensions(held.copy(generatedUi = !held.generatedUi)) }
                }
                ExtensionSwitch("Rich Markdown", "Tells agents which Markdown Mycelium renders: task lists, callouts, footnotes, highlighted code. Added to Claude Code and Codex sessions and to chats.", held.richMarkdown) {
                    attempt { extensions = store.api.saveExtensions(held.copy(richMarkdown = !held.richMarkdown)) }
                }
                ExtensionSwitch("Mycelium tools", "Tells agents they run inside Mycelium and when to use its recall, remember, walkthrough, propose_task and ask_question tools. Added to Claude Code and Codex sessions.", held.myceliumTools) {
                    attempt { extensions = store.api.saveExtensions(held.copy(myceliumTools = !held.myceliumTools)) }
                }
            }
        }

        Label("Node synchronisation", Modifier.semantics { heading() }, Tokens.Type.UiStrong)

        val nodes = state.nodes.values.sortedBy { it.name }

        Table(
            listOf(
                Col("Node", 120.dp, weight = 1f),
                Col("Holds", 72.dp, max = 160.dp),
                Col("Sessions on an older version", 140.dp, max = 320.dp, overflow = Overflow.Wrap, priority = 1),
                ActionColumn,
            ),
            nodes.size,
            { row, column ->
                val node = nodes[row]

                when (column) {
                    0 -> TableCell.Text(node.name)
                    1 -> TableCell.Text(heldVersion(node), Tokens.Type.Code, colors.textMuted)
                    2 -> TableCell.Text(olderSessions(node), color = colors.textMuted)
                    else -> if (node.instructionsUnmanaged.isEmpty()) {
                        TableCell.Text("")
                    } else {
                        TableCell.Slot {
                            RowMenu("instructions on ${node.name}") { close ->
                                for (file in node.instructionsUnmanaged) {
                                    MenuItem("Manage ${file.substringAfterLast('/')}", onClick = {
                                        close()
                                        attempt { store.api.takeOverInstructions(node.id) }
                                    })
                                }
                            }
                        }
                    }
                }
            },
            detail = { row ->
                for (file in nodes[row].instructionsUnmanaged) {
                    Label("Not managed, left as it is: $file", Modifier.padding(horizontal = Tokens.Space.S3), style = Tokens.Type.Meta, color = colors.textMuted)

                    Label("Your file is kept as ${file.substringAfterLast('/')}.bak.", Modifier.padding(horizontal = Tokens.Space.S3), style = Tokens.Type.Meta, color = colors.textMuted)
                }
            },
        )
    }
}
