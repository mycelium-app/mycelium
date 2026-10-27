package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
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
import mycelium.state.Choice
import mycelium.state.OpeningMinutes
import mycelium.state.Store
import mycelium.state.groupStatusText
import mycelium.state.hoursSummary

private fun folderSummary(group: Group, state: AppState): List<String> =
    group.folders.map { folder -> "${folder.nodeId?.let { state.nodes[it]?.name ?: "Unknown node" } ?: "Any node"} ${folder.prefix}" }

private const val GroupsList = "groups"

const val GroupInstructionsTab = "Instructions"
const val GroupHoursTab = "Hours"

private val GroupTabs = listOf(GroupInstructionsTab, "Folders", "Projects", GroupHoursTab)

@Composable
fun GroupsSettings(store: Store, openGroup: String?, onOpenGroup: (String?) -> Unit, startTab: String? = null, onOpenTimeAndFocus: () -> Unit = {}) {
    val colors = MyTheme.colors
    val state by store.state.collectAsState()
    var failure by remember { mutableStateOf<String?>(null) }
    var openingOutOfHours by remember { mutableStateOf(false) }
    val view = rememberHoursView(store)
    val platform = LocalPlatform.current
    var naming by remember { mutableStateOf(false) }
    var newName by remember { mutableStateOf("") }
    var renaming by remember { mutableStateOf<String?>(null) }
    var renamed by remember { mutableStateOf("") }
    var deleting by remember { mutableStateOf<String?>(null) }
    var layer by remember { mutableStateOf<InstructionLayer?>(null) }
    var newFolder by remember { mutableStateOf("") }
    var folderNode by remember { mutableStateOf<String?>(null) }
    var addingFolder by remember { mutableStateOf(false) }
    val attempt = rememberAttempt { failure = it }
    var query by remember { mutableStateOf("") }
    val groups = state.groups.values.filter { it.name.contains(query.trim(), ignoreCase = true) }.sortedBy { it.name.lowercase() }
    val loadFailures by store.loadFailures.collectAsState()
    val load = { store.load(GroupsList) { store.refreshGroups(); store.refreshProjects() } }
    val open = openGroup?.let { state.groups[it] }
    val forming = renaming?.let { state.groups[it] }
    val doomed = deleting?.let { state.groups[it] }

    LaunchedEffect(Unit) {
        load()
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
                Column(Modifier.weight(1f)) {
                    val projectCount = state.projects.values.count { it.groupId == open.id }

                    Label(open.name, Modifier.semantics { heading() }, Tokens.Type.Title)
                    Label(listOf(if (projectCount == 1) "1 project" else "$projectCount projects", groupStatusText(view, open, platform)).joinToString(" · "), style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
                }

                RowMenu("${open.name} group") { close ->
                    MenuItem("Rename group", onClick = { close(); renaming = open.id; renamed = open.name; deleting = null })
                    MenuItem("Delete group", onClick = { close(); deleting = open.id; renaming = null })
                }
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
            if (state.groups.isNotEmpty()) {
                ListSearch(query, { query = it }, "groups")
            }

            if (state.groups.isEmpty() && GroupsList in loadFailures) {
                LoadFailedState("groups", load)
            } else if (state.groups.isEmpty()) {
                EmptyState("No groups. Choose New group to keep instructions, memory and the sidebar filter apart for different kinds of work.")
            } else if (groups.isEmpty()) {
                EmptyState("No group matches.")
            } else {
                Table(
                    listOf(
                        Col("Group", 140.dp, weight = 1f),
                        Col("Folders", 100.dp, max = 360.dp, priority = 1),
                        Col("Projects", 72.dp, numeric = true),
                        Col("Hours", 120.dp, max = 240.dp, priority = 2),
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
                            3 -> TableCell.Text(hoursSummary(group.hours), color = colors.textMuted)
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
            var tab by remember(open.id) { mutableStateOf(startTab?.takeIf { it in GroupTabs } ?: GroupTabs.first()) }

            TabRow(GroupTabs, tab, { tab = it }, groupLabel = "Group sections")

            if (tab == GroupTabs[0]) {
                InstructionProposals(store, layer = open.id)

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

            }

            if (tab == GroupTabs[1]) {
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

                    MyButton({ addingFolder = true; newFolder = ""; folderNode = null }, text = "Add folder", icon = MyIcon.Plus, variant = ButtonVariant.Outline)
                }

            }

            if (tab == GroupTabs[2]) {
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

            if (tab == GroupTabs[3]) {
                val status = view.groups[open.id]

                Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
                    HoursEditor(open.name, open.hours) { hours -> attempt { store.saveGroupHours(open.id, hours) } }
                    Label("Outside these hours ${open.name} leaves the sidebar, every list and the group selector, and its notifications wait. Agents keep working. It is never open while Mycelium is closed.", Modifier, Tokens.Type.Meta, colors.textMuted)

                    FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                        MyButton(onOpenTimeAndFocus, text = "Time and focus", icon = MyIcon.Clock, muted = true)

                        if (status != null && !status.open) {
                            MyButton({ openingOutOfHours = true }, text = "Open for $OpeningMinutes minutes", muted = true)
                        }
                    }
                }
            }
        }
    }

    if (naming) {
        fun create() {
            attempt {
                val group = store.api.createGroup(newName)
                store.refreshGroups()
                onOpenGroup(group.id)
                naming = false
            }
        }

        RenameDialog("New group", "Group name", newName, { newName = it }, ::create, { naming = false })
    }

    if (forming != null) {
        fun rename() {
            attempt {
                store.api.renameGroup(forming.id, renamed)
                store.refreshGroups()
                renaming = null
            }
        }

        RenameDialog("Rename group", "New group name", renamed, { renamed = it }, ::rename, { renaming = null })
    }

    if (addingFolder && open != null) {
        fun addFolder() {
            attempt {
                store.api.addGroupFolder(open.id, newFolder, folderNode)
                store.refreshGroups()
                addingFolder = false
            }
        }

        Dialog({ addingFolder = false }) {
            Label("Add a folder", Modifier.semantics { heading() }, Tokens.Type.Title)
            TextInput(newFolder, { newFolder = it }, label = "Folder", placeholder = "/home/sam/code", mono = true, onSubmit = ::addFolder)
            OptionMenu("Node", listOf(Choice("", "Any node")) + state.nodes.values.sortedBy { it.name }.map { Choice(it.id, it.name) }, folderNode.orEmpty(), { folderNode = it.ifEmpty { null } })

            FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                MyButton({ addingFolder = false }, text = "Cancel", muted = true)
                MyButton(::addFolder, text = "Save", variant = ButtonVariant.Outline)
            }
        }
    }

    if (openingOutOfHours) {
        val closed = view.groups.values.filter { !it.open }.map { it.group }

        if (closed.isEmpty()) {
            openingOutOfHours = false
        } else {
            OpenOutOfHoursDialog(store, platform, closed, open?.id, onDismiss = { openingOutOfHours = false }, onOpened = {})
        }
    }
}
