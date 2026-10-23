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
fun InstructionsSettings(store: Store) {
    val colors = MyTheme.colors
    val scope = rememberCoroutineScope()
    val state by store.state.collectAsState()
    var document by remember { mutableStateOf<Instructions?>(null) }
    var forges by remember { mutableStateOf<List<Forge>>(emptyList()) }
    var extensions by remember { mutableStateOf<Extensions?>(null) }
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
    var newFolder by remember { mutableStateOf("") }
    var folderNode by remember { mutableStateOf<String?>(null) }
    var folderNodeMenu by remember { mutableStateOf(false) }

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
            extensions = store.api.extensions()
            store.refreshProjects()
            store.refreshGroups()
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
            FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2), itemVerticalAlignment = Alignment.CenterVertically) {
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

        if (layer != null && layer.layer != GlobalLayer) {
            val folders = state.groups[layer.layer]?.folders.orEmpty()

            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                Label("Folders in ${layer.name}", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
                Label("Conversations without a project, including imported ones, join ${layer.name} when their folder is inside one of these.", style = Tokens.Type.Meta, color = colors.textMuted)

                for (folder in folders) {
                    val nodeName = folder.nodeId?.let { state.nodes[it]?.name ?: "Unknown node" } ?: "Any node"

                    Row(Modifier.fillMaxWidth().heightIn(min = rowHeight()).padding(horizontal = Tokens.Space.S2), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3), verticalAlignment = Alignment.CenterVertically) {
                        Label(folder.prefix, Modifier.weight(1f), Tokens.Type.Code, maxLines = 1)
                        Label(nodeName, color = colors.textMuted, maxLines = 1)
                        MyButton(
                            { attempt { store.api.removeGroupFolder(layer.layer, folder.id); store.refreshGroups() } },
                            text = "Remove",
                            size = ButtonSize.Sm,
                            variant = ButtonVariant.Outline,
                            muted = true,
                            description = "Remove folder ${folder.prefix} from ${layer.name}",
                        )
                    }

                    Hairline()
                }

                Row(Modifier.widthIn(max = Tokens.Size.FormColumn).padding(top = Tokens.Space.S2), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.Bottom) {
                    fun addFolder() {
                        attempt {
                            store.api.addGroupFolder(layer.layer, newFolder, folderNode)
                            store.refreshGroups()
                            newFolder = ""
                        }
                    }

                    TextInput(newFolder, { newFolder = it }, label = "Folder", placeholder = "/home/sam/code", mono = true, modifier = Modifier.weight(1f), onSubmit = ::addFolder)

                    Box {
                        val chosen = state.nodes[folderNode]?.name ?: "Any node"

                        MyButton(
                            { folderNodeMenu = true },
                            text = chosen,
                            description = "Node: $chosen",
                            variant = ButtonVariant.Outline,
                            trailingIcon = MyIcon.ChevronDown,
                        )

                        if (folderNodeMenu) {
                            MenuPopup(onDismiss = { folderNodeMenu = false }) {
                                MenuLabel("Node")
                                MenuItem("Any node", onClick = { folderNode = null; folderNodeMenu = false }, checked = folderNode == null)

                                for (node in state.nodes.values.sortedBy { it.name }) {
                                    MenuItem(node.name, onClick = { folderNode = node.id; folderNodeMenu = false }, checked = node.id == folderNode)
                                }
                            }
                        }
                    }

                    MyButton(::addFolder, text = "Add folder", variant = ButtonVariant.Outline)
                }
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

        extensions?.let { held ->
            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                Label("Mycelium extensions", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
                Label("Built-in additions to the instructions of conversations that start after you change them.", style = Tokens.Type.Meta, color = colors.textMuted)

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

        ContentTable(listOf(ColumnSpec(max = 320.dp, flexible = true), ColumnSpec(), ColumnSpec(max = 320.dp))) {
            ContentRow(header = true) {
                Label("Node", style = Tokens.Type.MetaStrong, color = colors.textMuted)
                Label("Holds", style = Tokens.Type.MetaStrong, color = colors.textMuted)
                Label("Sessions on an older version", style = Tokens.Type.MetaStrong, color = colors.textMuted)
            }

            Hairline()

            for (node in state.nodes.values.sortedBy { it.name }) {
                Column(Modifier.fillMaxWidth().heightIn(min = rowHeight()).padding(vertical = Tokens.Space.S1)) {
                    ContentRow(minHeight = 0.dp) {
                        Label(node.name, maxLines = 1)
                        Label(heldVersion(node), style = Tokens.Type.Code, color = colors.textMuted, maxLines = 1)
                        Label(olderSessions(node), color = colors.textMuted)
                    }

                    for (file in node.instructionsUnmanaged) {
                        Label("Not managed, left as it is: $file", Modifier.padding(horizontal = Tokens.Space.S3), style = Tokens.Type.Meta, color = colors.textMuted)
                    }

                        Column(Modifier.padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S1)) {
                            MyButton(
                                { attempt { store.api.takeOverInstructions(node.id) } },
                                text = "Let Mycelium manage this",
                                description = "Let Mycelium manage this ${file.substringAfterLast('/')} on ${node.name}",
                                variant = ButtonVariant.Outline,
                                size = ButtonSize.Sm,
                            )
                            Label("Your file is kept as ${file.substringAfterLast('/')}.bak.", style = Tokens.Type.Meta, color = colors.textMuted)
                        }
                }

                Hairline()
            }
        }

        if (layer != null) {
            val viewportHeight = with(LocalDensity.current) { LocalWindowInfo.current.containerSize.height.toDp() }
            val editorCap = maxOf(Tokens.Size.InstructionsEditorCap, viewportHeight * 0.6f)

            Column(Modifier.fillMaxWidth().border(Tokens.Size.Hairline, colors.border, ControlShape)) {
                Row(
                    Modifier.fillMaxWidth().background(colors.bgPanel).padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2),
                    horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Label(layerFile(layer, source?.repository), Modifier.weight(1f), Tokens.Type.Code, maxLines = 1)
                    Label(layer.version?.let { "${shortVersion(it.id)} · ${relativeTime(it.createdAt)}" } ?: "No version yet", style = Tokens.Type.Meta, color = colors.textFaint, maxLines = 1)
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
                            .semantics { contentDescription = "Edit ${layer.name} instructions" },
                    )
                }
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
            }
        }
    }
}
