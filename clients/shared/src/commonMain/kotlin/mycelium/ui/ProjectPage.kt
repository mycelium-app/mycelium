package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import mycelium.api.NodeSummary
import mycelium.api.Project
import mycelium.api.ProjectCheckout
import mycelium.state.AppState
import mycelium.state.ColumnAlign
import mycelium.state.Store
import mycelium.state.checkedOutBranch

private val ProjectPageMeasure = 960.dp

@Composable
fun ProjectPage(store: Store, state: AppState, projectId: String, onBack: () -> Unit, onNewConversation: (String) -> Unit) {
    val colors = MyTheme.colors
    val project = state.projects[projectId]
    val forge = state.forges?.firstOrNull { it.id == project?.forgeId }
    var adding by remember { mutableStateOf(false) }
    var cloning by remember { mutableStateOf<Set<String>>(emptySet()) }
    var choosingFolderOn by remember { mutableStateOf<NodeSummary?>(null) }
    var removing by remember { mutableStateOf<ProjectCheckout?>(null) }
    var changingIcon by remember { mutableStateOf(false) }
    var changingGroup by remember { mutableStateOf(false) }
    var editingTestCommand by remember { mutableStateOf(false) }
    var confirmingRemove by remember { mutableStateOf(false) }
    val failures = rememberAction()

    LaunchedEffect(Unit) {
        store.load { store.refreshProjects() }
        store.load { store.refreshGroups() }
    }

    Column(Modifier.fillMaxSize()) {
        Bar {
            BackButton("Projects", onBack, showText = true)

            if (project == null) {
                Label("This project no longer exists.", color = colors.textMuted)
            } else {
                Label("/", color = colors.textFaint)
                ProjectIcon(project)
                Label(project.name, Modifier.semantics { heading() }, Tokens.Type.Title, maxLines = 1)

                project.repository?.let { repository ->
                    Label(forge?.let { "$repository on ${forgeProduct(it)}" } ?: repository, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
                }

                Box(Modifier.weight(1f))

                MyButton({ onNewConversation(project.id) }, text = "New conversation", variant = ButtonVariant.Outline, description = "New conversation in ${project.name}")

                RowMenu(project.name) { close ->
                    val archived = project.archivedAt != null

                    MenuItem(if (archived) "Restore" else "Archive", onClick = { close(); failures { store.archiveProject(project.id, !archived) } }, leading = MyIcon.Archive)

                    if (archived) {
                        MenuItem("Remove", onClick = { close(); confirmingRemove = true })
                    }
                }
            }
        }

        if (confirmingRemove && project != null) {
            ConfirmDialog(
                "Remove ${project.name}?",
                "Its schedules and tasks go with it. Its conversations stay, without a project, and its folders and files stay on their nodes.",
                "Remove project",
                onDismiss = { confirmingRemove = false },
            ) {
                confirmingRemove = false
                failures {
                    store.removeProject(project.id)
                    onBack()
                }
            }
        }

        if (project != null) {
            Column(Modifier.weight(1f).fillMaxWidth().verticalScrollWithBar().padding(Tokens.Space.S4), horizontalAlignment = Alignment.CenterHorizontally) {
                Column(Modifier.widthIn(max = ProjectPageMeasure).fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4)) {
                    failures.failure?.let { ErrorBanner(it, onDismiss = { failures.failure = null }) }

                    val places = project.places
                    val withoutCheckout = state.nodes.values.filter { node -> node.online && places.none { it.nodeId == node.id } }.sortedBy { it.name }
                    val canAdd = cloneable(project) && withoutCheckout.isNotEmpty()

                    Card {
                        CardHead {
                            Label("Checkouts", Modifier.weight(1f).semantics { heading() }, Tokens.Type.UiStrong)

                            if (canAdd) {
                                Box {
                                    MyButton({ adding = true }, text = "Add a checkout", icon = MyIcon.Plus, variant = ButtonVariant.Outline, size = ButtonSize.Sm, selected = adding)

                                    if (adding) {
                                        MenuPopup(onDismiss = { adding = false }) {
                                            MenuLabel("Nodes without a checkout")

                                            for (node in withoutCheckout) {
                                                MenuItem(
                                                    "Clone onto ${node.name}",
                                                    onClick = {
                                                        adding = false
                                                        cloning = cloning + node.id

                                                        store.act {
                                                            try {
                                                                store.addCheckout(project.id, node.id)
                                                            } finally {
                                                                cloning = cloning - node.id
                                                            }
                                                        }
                                                    },
                                                    hint = "Online",
                                                    leading = nodeIcon(node),
                                                    leadingColour = nodeColour(node),
                                                )

                                                MenuItem("Use a folder on ${node.name}", onClick = { adding = false; choosingFolderOn = node }, leading = MyIcon.Folder)
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        Table(
                            listOf(
                                Col("Node", 80.dp, weight = 1f),
                                Col("Folder", 120.dp, weight = 2f, priority = 1),
                                Col("Branch", 80.dp, max = 200.dp, priority = 1),
                                Col("Conversations", 100.dp, align = ColumnAlign.End, numeric = true),
                                ActionColumn,
                            ),
                            places.size,
                            { index, column ->
                                val place = places[index]
                                val node = state.nodes[place.nodeId]

                                when (column) {
                                    0 -> TableCell.Slot(node?.name.orEmpty(), Tokens.Size.IconMd) {
                                        Row(Modifier.semantics { contentDescription = "Checkout on ${node?.name ?: "an unknown node"}" }, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalAlignment = Alignment.CenterVertically) {
                                            if (node != null) {
                                                Icon(nodeIcon(node), nodeColour(node), Tokens.Size.IconMd)
                                            }

                                            Label(node?.name ?: "Unknown node", maxLines = 1)
                                        }
                                    }

                                    1 -> TableCell.Text(place.path, Tokens.Type.Code, colors.textMuted)
                                    2 -> checkedOutBranch(place, state.conversations.values)?.let { branch ->
                                        TableCell.Slot(measureAs = branch, extra = Tokens.Size.IconSm + Tokens.Space.S1) {
                                            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
                                                Icon(MyIcon.Branch, colors.textMuted, Tokens.Size.IconSm)
                                                Label(branch, color = colors.textMuted, maxLines = 1)
                                            }
                                        }
                                    } ?: TableCell.Text("")
                                    3 -> TableCell.Text(state.conversations.values.count { it.projectId == project.id && it.nodeId == place.nodeId }.toString(), color = colors.textMuted)
                                    else -> if (places.size > 1) {
                                        TableCell.Slot {
                                            RowMenu("the checkout on ${node?.name ?: "this node"}") { close ->
                                                MenuItem("Remove checkout", onClick = { close(); removing = place })
                                            }
                                        }
                                    } else {
                                        TableCell.Text("")
                                    }
                                }
                            },
                            card = false,
                            lazy = false,
                            rowDescription = { "Checkout on ${state.nodes[places[it].nodeId]?.name ?: "an unknown node"}" },
                        )

                        for (nodeId in cloning) {
                            Hairline()

                            Column(Modifier.padding(Tokens.Space.S2)) {
                                IndeterminateProgress("Cloning ${project.name} on ${state.nodes[nodeId]?.name.orEmpty()}")
                            }
                        }
                    }

                    Card {
                        CardHead {
                            Label("Settings", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
                        }

                        SettingLine("Icon", { ProjectIcon(project) }, "Change", "Change icon of ${project.name}") { changingIcon = true }
                        Hairline()

                        Box {
                            SettingLine("Group", { Label(state.groups[project.groupId]?.name ?: NoGroupName) }, "Change", "Change group of ${project.name}") { changingGroup = true }

                            if (changingGroup) {
                                MenuPopup(onDismiss = { changingGroup = false }) {
                                    MenuLabel("Group of ${project.name}")
                                    MenuItem(NoGroupName, onClick = { changingGroup = false; store.act { store.setProjectGroup(project.id, null) } }, checked = project.groupId == null)

                                    for (group in state.groups.values.sortedBy { it.name.lowercase() }) {
                                        MenuItem(group.name, onClick = { changingGroup = false; store.act { store.setProjectGroup(project.id, group.id) } }, checked = project.groupId == group.id)
                                    }
                                }
                            }
                        }

                        Hairline()

                        val testCommand = project.testCommand

                        SettingLine("Test command", { if (testCommand.isNullOrBlank()) Label("None", color = colors.textMuted) else Label(testCommand, style = Tokens.Type.Code) }, if (testCommand.isNullOrBlank()) "Set" else "Edit", "Edit test command of ${project.name}") { editingTestCommand = true }
                    }
                }
            }
        }
    }

    choosingFolderOn?.let { node ->
        project?.let { held -> CheckoutFolderDialog(store, held, node, onDismiss = { choosingFolderOn = null }) }
    }

    removing?.let { checkout ->
        project?.let { held -> RemoveCheckoutDialog(store, held, checkout, state.nodes[checkout.nodeId]?.name ?: "this node", onDismiss = { removing = null }) }
    }

    if (changingIcon && project != null) {
        ProjectIconDialog(store, listOf(project), project.name, onDismiss = { changingIcon = false })
    }

    if (editingTestCommand && project != null) {
        TestCommandDialog(store, project, onDismiss = { editingTestCommand = false })
    }
}

@Composable
private fun SettingLine(name: String, value: @Composable () -> Unit, action: String, description: String, onClick: () -> Unit) {
    Row(Modifier.fillMaxWidth().heightIn(min = Tokens.Size.ControlLg).padding(horizontal = Tokens.Space.S2), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
        Label(name, Modifier.weight(1f), color = MyTheme.colors.textMuted)
        Box(Modifier.weight(3f)) { value() }
        MyButton(onClick, text = action, description = description, size = ButtonSize.Sm, muted = true)
    }
}

@Composable
private fun CheckoutFolderDialog(store: Store, project: Project, node: NodeSummary, onDismiss: () -> Unit) {
    var folder by remember { mutableStateOf("") }
    val action = rememberAction()

    fun submit() {
        if (action.running) {
            return
        }

        if (folder.isBlank()) {
            action.failure = "Enter the folder on ${node.name}, for example /home/sam/projects/${project.repository.orEmpty()}."
            return
        }

        action {
            store.addCheckout(project.id, node.id, path = folder.trim())
            onDismiss()
        }
    }

    Dialog(onDismiss) {
        Label("Use a folder on ${node.name}", Modifier.semantics { heading() }, Tokens.Type.Title)
        Label("The folder must hold ${project.repository ?: project.name} already. Nothing is cloned.", style = Tokens.Type.Meta, color = MyTheme.colors.textMuted)
        FolderField(store, node, folder, { folder = it }, label = "Folder on ${node.name}", placeholder = "/home/sam/projects/acme-org/backups", onSubmit = ::submit)

        action.failure?.let { ErrorBanner(it, onDismiss = { action.failure = null }) }

        FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            MyButton(onDismiss, text = "Cancel", muted = true)
            MyButton(::submit, text = "Add checkout", variant = ButtonVariant.Outline)
        }
    }
}

@Composable
private fun RemoveCheckoutDialog(store: Store, project: Project, checkout: ProjectCheckout, nodeName: String, onDismiss: () -> Unit) {
    val action = rememberAction()

    Dialog(onDismiss) {
        Label("Remove the checkout on $nodeName?", Modifier.semantics { heading() }, Tokens.Type.Title)
        Label(
            "Conversations in ${project.name} stop starting on $nodeName. The folder ${checkout.path} and its files stay on $nodeName, and earlier conversations stay in ${project.name}.",
            style = Tokens.Type.Meta,
            color = MyTheme.colors.textMuted,
        )

        action.failure?.let { ErrorBanner(it, onDismiss = { action.failure = null }) }

        FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            MyButton(onDismiss, text = "Cancel", muted = true)
            MyButton(
                {
                    if (!action.running) {
                        action {
                            store.removeCheckout(project.id, checkout.nodeId)
                            onDismiss()
                        }
                    }
                },
                text = "Remove checkout",
                variant = ButtonVariant.Danger,
            )
        }
    }
}

@Composable
private fun TestCommandDialog(store: Store, project: Project, onDismiss: () -> Unit) {
    var command by remember { mutableStateOf(project.testCommand.orEmpty()) }
    val action = rememberAction()

    fun save() {
        if (action.running) {
            return
        }

        action {
            store.setProjectTestCommand(project.id, command.trim())
            onDismiss()
        }
    }

    Dialog(onDismiss) {
        Label("Test command for ${project.name}", Modifier.semantics { heading() }, Tokens.Type.Title)
        TextInput(command, { command = it }, label = "Test command", placeholder = "make test", mono = true, onSubmit = ::save)

        action.failure?.let { ErrorBanner(it, onDismiss = { action.failure = null }) }

        FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            MyButton(onDismiss, text = "Cancel", muted = true)
            MyButton(::save, text = "Save", variant = ButtonVariant.Outline)
        }
    }
}
