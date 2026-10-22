package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.unit.dp
import androidx.compose.ui.semantics.semantics
import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.api.ConfigRequest
import mycelium.api.LoginInfo
import mycelium.api.NodeSummary
import mycelium.api.PatchConversationRequest
import mycelium.state.AppState
import mycelium.state.Store
import mycelium.state.agentName
import mycelium.state.isLaptop
import mycelium.state.buildThread
import mycelium.state.loginMethodName
import mycelium.state.projectName
import mycelium.state.relativeTime
import mycelium.state.statusSectionName

@Composable
fun ConversationScreen(store: Store, state: AppState, id: String, onArchived: () -> Unit, onOpenNodes: () -> Unit = {}) {
    val api = store.api
    var fetched by remember(id) { mutableStateOf<Conversation?>(null) }
    var inspectorOpen by remember { mutableStateOf(false) }
    var nodeSheetOpen by remember { mutableStateOf(false) }
    val compact = LocalTouch.current
    val conversation = state.conversations[id] ?: fetched

    LaunchedEffect(id) {
        store.act { store.open(id) }

        if (state.conversations[id] == null) {
            store.act { fetched = api.conversation(id) }
        }
    }

    if (conversation == null) {
        Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
            Label("Loading the conversation", color = MyTheme.colors.textMuted)
        }

        return
    }

    val node = state.nodes[conversation.nodeId]
    val thread = remember(state.events[id]) { buildThread(state.events[id].orEmpty()) }
    val agentLine = listOfNotNull(agentName(conversation.agent), conversation.model).joinToString(" · ")

    Row(Modifier.fillMaxSize()) {
        Column(Modifier.weight(1f).fillMaxHeight()) {
            ConversationHeader(
                conversation = conversation,
                node = node,
                compact = compact,
                onNode = { nodeSheetOpen = true },
                onDetails = { inspectorOpen = true },
                onRename = { title -> store.act { fetched = api.patchConversation(id, PatchConversationRequest(title = title)) } },
                onArchive = {
                    store.act {
                        api.patchConversation(id, PatchConversationRequest(archived = conversation.archivedAt == null))
                        onArchived()
                    }
                },
            )

            ThreadView(
                items = thread,
                agentLine = agentLine,
                onAnswer = { requestId, optionId -> store.act { api.answerPermission(id, requestId, optionId) } },
                onStopWorkflow = { store.act { api.cancel(id) } },
                modifier = Modifier.weight(1f),
            )

            Composer(
                conversation = conversation,
                working = conversation.status == ConversationStatus.Working,
                queue = state.queues[id].orEmpty(),
                onSend = { text -> store.act { api.sendPrompt(id, text) } },
                onStop = { store.act { api.cancel(id) } },
                onSendQueuedNow = { queueId -> store.act { api.sendQueuedNow(id, queueId) } },
                onEditQueued = { queueId, text -> store.act { api.editQueued(id, queueId, text) } },
                onRemoveQueued = { queueId -> store.act { api.removeQueued(id, queueId) } },
                onSetMode = { mode -> store.act { api.setConfig(id, ConfigRequest(mode = mode)) } },
                onSetModel = { model -> store.act { api.setConfig(id, ConfigRequest(model = model)) } },
            )
        }

        if (!compact) {
            Rail(inspectorOpen) { inspectorOpen = !inspectorOpen }

            if (inspectorOpen) {
                Inspector(conversation, node)
            }
        }
    }

    if (compact && inspectorOpen) {
        BottomSheet(onDismiss = { inspectorOpen = false }) {
            InspectorContent(conversation, node)
        }
    }

    if (compact && nodeSheetOpen) {
        NodeSheet(node, onDismiss = { nodeSheetOpen = false }, onOpenNodes = { nodeSheetOpen = false; onOpenNodes() })
    }
}

@Composable
private fun ConversationHeader(
    conversation: Conversation,
    node: NodeSummary?,
    compact: Boolean,
    onNode: () -> Unit,
    onDetails: () -> Unit,
    onRename: (String) -> Unit,
    onArchive: () -> Unit,
) {
    val colors = MyTheme.colors
    var renaming by remember(conversation.id) { mutableStateOf(false) }
    var draft by remember(conversation.title) { mutableStateOf(conversation.title) }
    var menuOpen by remember { mutableStateOf(false) }
    val archived = conversation.archivedAt != null

    Bar(startPadding = if (compact) Tokens.Space.S1 else Tokens.Space.S3) {
        LocalOpenList.current?.let { openList ->
            MyButton(openList, icon = MyIcon.Menu, description = "Conversations", muted = true)
        }

        if (renaming) {
            TextInput(draft, { draft = it }, label = "Conversation name", modifier = if (compact) Modifier.weight(1f) else Modifier.width(Tokens.Size.FormColumn), showLabel = false, onSubmit = {
                if (draft.isNotBlank()) {
                    onRename(draft.trim())
                    renaming = false
                }
            })
            MyButton({ if (draft.isNotBlank()) { onRename(draft.trim()); renaming = false } }, text = "Save", size = ButtonSize.Sm, variant = ButtonVariant.Outline)
            MyButton({ renaming = false; draft = conversation.title }, text = "Cancel", size = ButtonSize.Sm, muted = true)
        } else {
            Row(
                (if (compact) Modifier.weight(1f) else Modifier.widthIn(max = Tokens.Size.Measure / 2))
                    .heightIn(min = touchMin(0.dp))
                    .pressable(
                        { if (compact) menuOpen = true else renaming = true },
                        description = if (compact) "${conversation.title}, rename or archive" else "Rename conversation ${conversation.title}",
                        shape = ControlShape,
                    )
                    .padding(horizontal = Tokens.Space.S1),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Label(conversation.title, style = Tokens.Type.Title, maxLines = 1)
            }
        }

        if (compact) {
            MyButton(onNode, icon = if (node != null && isLaptop(node.os)) MyIcon.Laptop else MyIcon.Server, description = "Running on ${node?.name ?: "an unknown node"}", muted = true)
            MyButton(onDetails, icon = MyIcon.Info, description = "Details", muted = true)
        } else {
            Label(
                listOfNotNull(projectName(conversation.cwd), node?.name).joinToString("  ·  "),
                style = Tokens.Type.Meta,
                color = colors.textMuted,
                maxLines = 1,
            )

            if (archived) {
                Tag("Archived")
            } else {
                StatusMark(statusSectionName(conversation.status), toneOf(conversation.status), iconOf(conversation.status))
            }

            Box(Modifier.weight(1f))
        }

        Box {
            if (!compact) {
                MyButton({ menuOpen = true }, icon = MyIcon.More, description = "More", muted = true)
            }

            if (menuOpen) {
                MenuPopup(onDismiss = { menuOpen = false }) {
                    if (compact) {
                        MenuItem("Rename", onClick = { menuOpen = false; renaming = true }, leading = MyIcon.Pencil)
                    }

                    MenuItem(if (archived) "Restore" else "Archive", onClick = { menuOpen = false; onArchive() }, leading = MyIcon.Archive)
                }
            }
        }
    }
}

fun toneOf(status: ConversationStatus): Tone = when (status) {
    ConversationStatus.Working -> Tone.Working
    ConversationStatus.Input, ConversationStatus.Review -> Tone.Attention
    ConversationStatus.Failed -> Tone.Failed
    ConversationStatus.Done -> Tone.Muted
}

fun iconOf(status: ConversationStatus): MyIcon = when (status) {
    ConversationStatus.Working -> MyIcon.Working
    ConversationStatus.Input -> MyIcon.Alert
    ConversationStatus.Review -> MyIcon.Review
    ConversationStatus.Failed -> MyIcon.Failed
    ConversationStatus.Done -> MyIcon.Check
}

@Composable
private fun Rail(open: Boolean, onToggle: () -> Unit) {
    val colors = MyTheme.colors

    Row(Modifier.fillMaxHeight()) {
        VerticalHairline()

        Column(
            Modifier.width(Tokens.Size.Rail).fillMaxHeight().background(colors.bgChrome).padding(top = Tokens.Space.S1),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            MyButton(onToggle, icon = MyIcon.Info, description = if (open) "Close details" else "Open details", muted = true, selected = open)
        }
    }
}

@Composable
private fun Inspector(conversation: Conversation, node: NodeSummary?) {
    val colors = MyTheme.colors

    Row(Modifier.fillMaxHeight()) {
        VerticalHairline()

        Column(Modifier.width(Tokens.Size.Inspector).fillMaxHeight().background(colors.bgPanel).semantics { contentDescription = "Conversation details" }) {
            Bar {
                BarTitle("Details")
            }

            Column(Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(top = Tokens.Space.S2, bottom = Tokens.Space.S4)) {
                InspectorContent(conversation, node)
            }
        }
    }
}

@Composable
private fun InspectorContent(conversation: Conversation, node: NodeSummary?) {
    val agentInfo = node?.agents?.firstOrNull { it.agent == conversation.agent }
    val login: LoginInfo? = node?.logins?.firstOrNull { it.agent == conversation.agent }

    InspectorSection(
        "Runs on",
        listOf(
            "Node" to (node?.name ?: "Unknown"),
            "Agent" to listOfNotNull(agentName(conversation.agent), agentInfo?.version).joinToString(" "),
            "Account" to (login?.account ?: "Not signed in"),
            "Login" to (login?.let { loginMethodName(it.method) } ?: "None"),
        ),
    )
    InspectorSection(
        "Conversation",
        listOf(
            "Folder" to conversation.cwd,
            "Status" to statusSectionName(conversation.status),
            "Mode" to (conversation.mode ?: "Default"),
            "Model" to (conversation.model ?: "Not set"),
            "Started" to relativeTime(conversation.createdAt).let { if (it == "now") it else "$it ago" },
        ),
    )
}

@Composable
private fun NodeSheet(node: NodeSummary?, onDismiss: () -> Unit, onOpenNodes: () -> Unit) {
    BottomSheet(onDismiss) {
        MenuLabel("Running on")
        InspectorSection(
            node?.name ?: "Unknown node",
            listOf(
                "State" to if (node?.online == true) "Online" else "Offline",
                "System" to listOfNotNull(node?.os, node?.arch).joinToString(" "),
                "Agents" to node?.agents?.filter { it.available }?.joinToString(", ") { agentName(it.agent) }.orEmpty().ifEmpty { "None available" },
            ),
        )
        MenuItem("All nodes", onClick = onOpenNodes, leading = MyIcon.Server)
    }
}

@Composable
private fun InspectorSection(name: String, rows: List<Pair<String, String>>) {
    val colors = MyTheme.colors

    Column(Modifier.fillMaxWidth().padding(bottom = Tokens.Space.S4)) {
        SectionHead(name)

        for ((label, value) in rows) {
            Row(
                Modifier
                    .fillMaxWidth()
                    .heightIn(min = rowHeight())
                    .padding(horizontal = Tokens.Space.S3)
                    .semantics(mergeDescendants = true) { contentDescription = "$label: $value" },
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Label(label, Modifier.weight(1f), color = colors.textMuted, maxLines = 1)
                Label(value, Modifier.weight(2f), Tokens.Type.Meta, maxLines = 2)
            }
        }
    }
}
