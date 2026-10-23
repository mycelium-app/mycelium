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
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
import mycelium.api.ApiException
import mycelium.api.Forge
import mycelium.api.Instructions
import mycelium.api.SetInstructionSourceRequest
import mycelium.state.Store
import mycelium.state.heldVersion
import mycelium.state.layerFile
import mycelium.state.olderSessions
import mycelium.state.relativeTime
import mycelium.state.shortVersion

private const val GlobalLayer = "global"

private fun forgeLabel(forge: Forge): String = "${if (forge.kind == "github") "GitHub" else "GitLab"} ${forge.account}"

@Composable
fun InstructionsSettings(store: Store) {
    val colors = MyTheme.colors
    val scope = rememberCoroutineScope()
    val state by store.state.collectAsState()
    var document by remember { mutableStateOf<Instructions?>(null) }
    var forges by remember { mutableStateOf<List<Forge>>(emptyList()) }
    var selected by remember { mutableStateOf(GlobalLayer) }
    var draft by remember { mutableStateOf("") }
    var failure by remember { mutableStateOf<String?>(null) }
    var naming by remember { mutableStateOf<String?>(null) }
    var newName by remember { mutableStateOf("") }
    var renaming by remember { mutableStateOf(false) }
    var renamed by remember { mutableStateOf("") }
    var deleting by remember { mutableStateOf(false) }
    var choosingSource by remember { mutableStateOf(false) }
    var sourceForge by remember { mutableStateOf<String?>(null) }
    var sourceRepository by remember { mutableStateOf("") }
    var sourcePath by remember { mutableStateOf("") }
    var forgeMenu by remember { mutableStateOf(false) }

    val layers = document?.layers.orEmpty()
    val layer = layers.firstOrNull { it.layer == selected } ?: layers.firstOrNull()
    val source = document?.source
    val saved = layer?.text.orEmpty()

    fun attempt(block: suspend () -> Unit) {
        scope.launch {
            try {
                block()
                failure = null
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (problem: ApiException) {
                failure = problem.message
            } catch (_: Exception) {
                failure = "Could not reach the control plane. Check your connection and try again."
            }
        }
    }

    LaunchedEffect(Unit) {
        store.act {
            document = store.api.instructions()
            forges = store.api.forges()
            store.refreshProjects()
        }
    }

    LaunchedEffect(layer?.layer, layer?.version?.id) {
        draft = saved
    }

    Column(Modifier.widthIn(max = Tokens.Size.Page), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4)) {
        Label("Instructions", Modifier.semantics { heading() }, Tokens.Type.UiStrong)

        failure?.let { ErrorBanner(it, onDismiss = { failure = null }) }

        FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), itemVerticalAlignment = Alignment.CenterVertically) {
            for (entry in layers) {
                MyButton(
                    { selected = entry.layer; renaming = false; deleting = false },
                    text = entry.name,
                    icon = if (entry.layer == GlobalLayer) MyIcon.Layers else null,
                    variant = ButtonVariant.Outline,
                    size = ButtonSize.Sm,
                    muted = entry.layer != layer?.layer,
                    selected = entry.layer == layer?.layer,
                    description = if (entry.layer == GlobalLayer) "Global instructions" else "Instructions for the ${entry.name} group",
                )
            }

            MyButton({ naming = ""; newName = "" }, icon = MyIcon.Plus, description = "New group", size = ButtonSize.Sm, muted = true)
        }

        if (naming != null) {
            Row(Modifier.widthIn(max = Tokens.Size.FormColumn), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.Bottom) {
                fun create() {
                    attempt {
                        val group = store.api.createGroup(newName)
                        document = store.api.instructions()
                        selected = group.id
                        naming = null
                    }
                }

                TextInput(newName, { newName = it }, label = "Group name", placeholder = "slop", modifier = Modifier.weight(1f), onSubmit = ::create)
                MyButton(::create, text = "Create group", variant = ButtonVariant.Outline)
                MyButton({ naming = null }, text = "Cancel", muted = true)
            }
        }

        if (layer != null) {
            Column(Modifier.fillMaxWidth().border(Tokens.Size.Hairline, colors.border, ControlShape)) {
                Row(
                    Modifier.fillMaxWidth().background(colors.bgPanel).padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2),
                    horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Label(layerFile(layer, source?.repository), Modifier.weight(1f), Tokens.Type.Code, maxLines = 1)
                    Label(layer.version?.let { "${shortVersion(it.id)} · ${relativeTime(it.createdAt)}" } ?: "No version yet", style = Tokens.Type.Meta, color = colors.textFaint, maxLines = 1)
                }

                BasicTextField(
                    value = draft,
                    onValueChange = { draft = it },
                    textStyle = Tokens.Type.Code.themed().copy(color = colors.text),
                    cursorBrush = SolidColor(colors.text),
                    modifier = Modifier
                        .fillMaxWidth()
                        .heightIn(min = Tokens.Size.InstructionsEditor)
                        .padding(Tokens.Space.S3)
                        .semantics { contentDescription = "Edit ${layer.name} instructions" },
                )
            }

            FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2), itemVerticalAlignment = Alignment.CenterVertically) {
                MyButton(
                    { attempt { store.api.saveInstructions(layer.layer, draft); document = store.api.instructions() } },
                    text = if (source != null) "Save and commit" else "Save",
                    variant = ButtonVariant.Outline,
                )

                if (source != null) {
                    MyButton({ attempt { document = store.api.pullInstructions() } }, text = "Pull", muted = true)
                }

                MyButton(
                    {
                        choosingSource = !choosingSource
                        sourceForge = source?.forgeId ?: forges.firstOrNull()?.id
                        sourceRepository = source?.repository.orEmpty()
                        sourcePath = source?.path.orEmpty()
                    },
                    text = if (source != null) "Change repository" else "Connect a repository",
                    muted = true,
                )

                if (layer.layer != GlobalLayer) {
                    MyButton({ renaming = !renaming; renamed = layer.name; deleting = false }, text = "Rename group", muted = true)
                    MyButton({ deleting = !deleting; renaming = false }, text = "Delete group", muted = true)
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

                        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
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

            if (renaming && layer.layer != GlobalLayer) {
                Row(Modifier.widthIn(max = Tokens.Size.FormColumn), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.Bottom) {
                    fun rename() {
                        attempt {
                            store.api.renameGroup(layer.layer, renamed)
                            document = store.api.instructions()
                            renaming = false
                        }
                    }

                    TextInput(renamed, { renamed = it }, label = "New group name", modifier = Modifier.weight(1f), onSubmit = ::rename)
                    MyButton(::rename, text = "Rename", variant = ButtonVariant.Outline)
                }
            }

            if (deleting && layer.layer != GlobalLayer) {
                Row(Modifier.fillMaxWidth().heightIn(min = rowHeight()), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                    Label("Delete the ${layer.name} group? Its projects stay, without a group, and its instructions stop reaching new conversations.", Modifier.weight(1f), color = colors.statusFailed)
                    MyButton(
                        {
                            attempt {
                                store.api.deleteGroup(layer.layer)
                                selected = GlobalLayer
                                deleting = false
                                document = store.api.instructions()
                                store.refreshProjects()
                            }
                        },
                        text = "Delete ${layer.name}",
                        variant = ButtonVariant.Danger,
                        size = ButtonSize.Sm,
                    )
                    MyButton({ deleting = false }, text = "Cancel", size = ButtonSize.Sm, muted = true)
                }
            }
        }

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            Row(Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S2), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
                Label("Node", Modifier.weight(2f), Tokens.Type.MetaStrong, colors.textMuted)
                Label("Holds", Modifier.weight(1.5f), Tokens.Type.MetaStrong, colors.textMuted)
                Label("Sessions on an older version", Modifier.weight(2.5f), Tokens.Type.MetaStrong, colors.textMuted)
            }

            Hairline()

            for (node in state.nodes.values.sortedBy { it.name }) {
                Column(Modifier.fillMaxWidth().heightIn(min = rowHeight()).padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1)) {
                    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3), verticalAlignment = Alignment.CenterVertically) {
                        Label(node.name, Modifier.weight(2f), maxLines = 1)
                        Label(heldVersion(node), Modifier.weight(1.5f), Tokens.Type.Code, colors.textMuted, maxLines = 1)
                        Label(olderSessions(node), Modifier.weight(2.5f), color = colors.textMuted)
                    }

                    for (file in node.instructionsUnmanaged) {
                        Label("Not managed, left as it is: $file", style = Tokens.Type.Meta, color = colors.textMuted)
                    }
                }

                Hairline()
            }
        }

        if (layer != null && layer.layer != GlobalLayer) {
            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                Label("Projects in ${layer.name}", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
                Label("Conversations in these projects start with this group's instructions after the global file. Claude Code only.", style = Tokens.Type.Meta, color = colors.textMuted)

                for (project in state.projects.values.sortedBy { it.name }) {
                    val member = project.groupId == layer.layer

                    Row(Modifier.fillMaxWidth().heightIn(min = rowHeight()).padding(horizontal = Tokens.Space.S2), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3), verticalAlignment = Alignment.CenterVertically) {
                        Label(project.name, Modifier.weight(1f), maxLines = 1)
                        Label(state.nodes[project.nodeId]?.name.orEmpty(), color = colors.textMuted, maxLines = 1)
                        MyButton(
                            { store.act { store.setProjectGroup(project.id, if (member) null else layer.layer) } },
                            text = if (member) "Remove" else "Add",
                            size = ButtonSize.Sm,
                            variant = ButtonVariant.Outline,
                            muted = !member,
                            description = if (member) "Remove ${project.name} from ${layer.name}" else "Add ${project.name} to ${layer.name}",
                        )
                    }

                    Hairline()
                }
            }
        }
    }
}
