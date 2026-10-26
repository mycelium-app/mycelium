package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import mycelium.api.Group
import mycelium.api.InstructionLayer
import mycelium.state.AppState
import mycelium.state.Store

private fun folderSummary(group: Group, state: AppState): List<String> =
    group.folders.map { folder -> "${folder.nodeId?.let { state.nodes[it]?.name ?: "Unknown node" } ?: "Any node"} ${folder.prefix}" }

@Composable
fun GroupsSettings(store: Store, openGroup: String?, onOpenGroup: (String?) -> Unit) {
    val colors = MyTheme.colors
    val state by store.state.collectAsState()
    var failure by remember { mutableStateOf<String?>(null) }
    var naming by remember { mutableStateOf(false) }
    var newName by remember { mutableStateOf("") }
    var renaming by remember { mutableStateOf<String?>(null) }
    var renamed by remember { mutableStateOf("") }
    var deleting by remember { mutableStateOf<String?>(null) }
    var layer by remember { mutableStateOf<InstructionLayer?>(null) }
    var newFolder by remember { mutableStateOf("") }
    var folderNode by remember { mutableStateOf<String?>(null) }
    var folderNodeMenu by remember { mutableStateOf(false) }
    val attempt = rememberAttempt { failure = it }
    val groups = state.groups.values.sortedBy { it.name.lowercase() }
    val open = openGroup?.let { state.groups[it] }
    val forming = renaming?.let { state.groups[it] }
    val doomed = deleting?.let { state.groups[it] }

    LaunchedEffect(Unit) {
        store.act {
            store.refreshGroups()
            store.refreshProjects()
        }
    }

    LaunchedEffect(open?.id) {
        layer = null

        if (open != null) {
            attempt { layer = store.api.groupInstructions(open.id) }
        }
    }

    Column(Modifier.widthIn(max = Tokens.Size.Page), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4)) {
        failure?.let { ErrorBanner(it, onDismiss = { failure = null }) }

        if (open == null) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Label("Groups", Modifier.weight(1f).semantics { heading() }, Tokens.Type.Title)
                MyButton({ naming = true; newName = "" }, text = "New group", icon = MyIcon.Plus, variant = ButtonVariant.Outline)
            }
        } else {
            BackButton("groups", { onOpenGroup(null) }, showText = true)

            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Label(open.name, Modifier.weight(1f).semantics { heading() }, Tokens.Type.Title)

                RowMenu("${open.name} group") { close ->
                    MenuItem("Rename group", onClick = { close(); renaming = open.id; renamed = open.name; deleting = null })
                    MenuItem("Delete group", onClick = { close(); deleting = open.id; renaming = null })
                }
            }
        }

        if (naming) {
            Row(Modifier.widthIn(max = Tokens.Size.FormColumn), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.Bottom) {
                fun create() {
                    attempt {
                        val group = store.api.createGroup(newName)
                        store.refreshGroups()
                        onOpenGroup(group.id)
                        naming = false
                    }
                }

                TextInput(newName, { newName = it }, label = "Group name", placeholder = "slop", modifier = Modifier.weight(1f), onSubmit = ::create)
                MyButton(::create, text = "Create group", variant = ButtonVariant.Outline)
                MyButton({ naming = false }, text = "Cancel", muted = true)
            }
        }

        if (forming != null) {
            Row(Modifier.widthIn(max = Tokens.Size.FormColumn), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.Bottom) {
                fun rename() {
                    attempt {
                        store.api.renameGroup(forming.id, renamed)
                        store.refreshGroups()
                        renaming = null
                    }
                }

                TextInput(renamed, { renamed = it }, label = "New group name", modifier = Modifier.weight(1f), onSubmit = ::rename)
                MyButton(::rename, text = "Rename", variant = ButtonVariant.Outline)
                MyButton({ renaming = null }, text = "Cancel", muted = true)
            }
        }

        if (doomed != null) {
            Row(Modifier.fillMaxWidth().heightIn(min = rowHeight()), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                Label("Delete the ${doomed.name} group? Its projects stay, without a group, and its instructions stop reaching new conversations.", Modifier.weight(1f), color = colors.statusFailed)
                MyButton(
                    {
                        attempt {
                            store.api.deleteGroup(doomed.id)
                            deleting = null
                            onOpenGroup(null)
                            store.refreshGroups()
                            store.refreshProjects()
                        }
                    },
                    text = "Delete ${doomed.name}",
                    variant = ButtonVariant.Danger,
                    size = ButtonSize.Sm,
                )
                MyButton({ deleting = null }, text = "Cancel", size = ButtonSize.Sm, muted = true)
            }
        }

        if (open == null) {
            if (groups.isEmpty()) {
                Label("Groups keep instructions, memory and the sidebar filter apart for different kinds of work.", color = colors.textMuted)
            } else {
                Table(
                    listOf(
                        Col("Group", 140.dp, weight = 1f),
                        Col("Folders", 100.dp, max = 360.dp, priority = 1),
                        Col("Projects", 72.dp, numeric = true),
                        ActionColumn,
                    ),
                    groups.size,
                    { row, column ->
                        val group = groups[row]
                        val folders = folderSummary(group, state)

                        when (column) {
                            0 -> TableCell.Slot(group.name) {
                                Label(group.name, Modifier.pressable({ onOpenGroup(group.id) }, role = Role.Button, description = "Open the ${group.name} group", shape = BlockShape).padding(vertical = Tokens.Space.S1), maxLines = 1)
                            }

                            1 -> if (folders.isEmpty()) {
                                TableCell.Text("None", color = colors.textFaint)
                            } else {
                                TableCell.Text(if (folders.size > 1) "${folders.first()} +${folders.size - 1} more" else folders.first(), Tokens.Type.Code)
                            }

                            2 -> TableCell.Text(state.projects.values.count { it.groupId == group.id }.toString(), color = colors.textMuted)
                            else -> TableCell.Slot {
                                RowMenu("${group.name} group") { close ->
                                    MenuItem("Rename", onClick = { close(); renaming = group.id; renamed = group.name; deleting = null })
                                    MenuItem("Delete", onClick = { close(); deleting = group.id; renaming = null })
                                }
                            }
                        }
                    },
                )
            }
        } else {
            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
                Label("Instructions", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
                Label("Added after the global instructions, for every conversation in ${open.name}.", style = Tokens.Type.Meta, color = colors.textMuted)

                layer?.let { held ->
                    InstructionsEditor(
                        layer = held,
                        file = held.path ?: "Stored in Mycelium",
                        saveLabel = if (held.path != null) "Save and commit" else "Save",
                        onSave = { text -> attempt { layer = store.api.saveGroupInstructions(open.id, text) } },
                    )
                }
            }

            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                Label("Folders in ${open.name}", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
                Label("Conversations without a project, including imported ones, join ${open.name} when their folder is inside one of these.", style = Tokens.Type.Meta, color = colors.textMuted)

                for (folder in open.folders) {
                    val nodeName = folder.nodeId?.let { state.nodes[it]?.name ?: "Unknown node" } ?: "Any node"

                    Row(Modifier.fillMaxWidth().heightIn(min = rowHeight()).padding(horizontal = Tokens.Space.S2), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3), verticalAlignment = Alignment.CenterVertically) {
                        Label(folder.prefix, Modifier.weight(1f), Tokens.Type.Code, maxLines = 1)
                        Label(nodeName, color = colors.textMuted, maxLines = 1)
                        RowMenu("folder ${folder.prefix} in ${open.name}") { close ->
                            MenuItem("Remove folder", onClick = {
                                close()
                                attempt { store.api.removeGroupFolder(open.id, folder.id); store.refreshGroups() }
                            })
                        }
                    }

                    Hairline()
                }

                Row(Modifier.widthIn(max = Tokens.Size.FormColumn).padding(top = Tokens.Space.S2), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.Bottom) {
                    fun addFolder() {
                        attempt {
                            store.api.addGroupFolder(open.id, newFolder, folderNode)
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

            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                Label("Projects in ${open.name}", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
                Label("Set a project's group from Projects.", style = Tokens.Type.Meta, color = colors.textMuted)

                for (project in state.projects.values.filter { it.groupId == open.id }.sortedBy { it.name }) {
                    Row(Modifier.fillMaxWidth().heightIn(min = rowHeight()).padding(horizontal = Tokens.Space.S2), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3), verticalAlignment = Alignment.CenterVertically) {
                        Label(project.name, Modifier.weight(1f), maxLines = 1)
                        Label(state.nodes[project.nodeId]?.name.orEmpty(), color = colors.textMuted, maxLines = 1)
                    }

                    Hairline()
                }
            }
        }
    }
}
