package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
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
import mycelium.state.ThreadItem
import mycelium.state.ago
import mycelium.state.DefaultAccountId
import mycelium.state.SignInPrefix
import mycelium.state.accountChoices
import mycelium.state.accountsElsewhere
import mycelium.state.agentName
import mycelium.state.switchableAccounts
import mycelium.state.isLaptop
import mycelium.state.buildThread
import mycelium.state.sessionOptions
import mycelium.state.slashCommands
import kotlinx.serialization.json.JsonPrimitive
import mycelium.state.loginMethodName
import mycelium.state.projectName
import mycelium.state.relativeTime
import mycelium.state.statusSectionName

@Composable
fun ConversationScreen(store: Store, state: AppState, id: String, onArchived: () -> Unit, onOpenNodes: () -> Unit = {}) {
    val api = store.api
    var fetched by remember(id) { mutableStateOf<Conversation?>(null) }
    var panel by remember { mutableStateOf<SidePanel?>(null) }
    var nodeSheetOpen by remember { mutableStateOf(false) }
    var signIn by remember { mutableStateOf<LoginSession?>(null) }
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

    LaunchedEffect(id, conversation.status) {
        if (conversation.status == ConversationStatus.Review) {
            store.act { api.markOpened(id) }
        }
    }

    val node = state.nodes[conversation.nodeId]
    val thread = remember(state.events[id]) { buildThread(state.events[id].orEmpty()) }
    val sending = state.sending[id].orEmpty()
    val goesStraight = conversation.status in setOf(ConversationStatus.Review, ConversationStatus.Done) && state.queues[id].isNullOrEmpty()
    val delivering = sending.take(if (goesStraight) 1 else 0)

    Row(Modifier.fillMaxSize()) {
        Column(Modifier.weight(1f).fillMaxHeight()) {
            ConversationHeader(
                conversation = conversation,
                node = node,
                compact = compact,
                onNode = { nodeSheetOpen = true },
                onDetails = { panel = SidePanel.Details },
                onTerminal = { panel = SidePanel.Terminal },
                onGit = { panel = SidePanel.Git },
                onRename = { title -> store.rename(conversation, title) },
                onArchive = {
                    store.act {
                        api.patchConversation(id, PatchConversationRequest(archived = conversation.archivedAt == null))
                        onArchived()
                    }
                },
            )

            ThreadView(
                items = thread + delivering.map { ThreadItem.UserMessage("sending", it) },
                onAnswer = { requestId, optionId -> store.act { api.answerPermission(id, requestId, optionId) } },
                onStopWorkflow = { store.act { api.cancel(id) } },
                modifier = Modifier.weight(1f),
            )

            Composer(
                conversation = conversation,
                working = conversation.status == ConversationStatus.Working,
                queue = state.queues[id].orEmpty(),
                sending = sending.drop(delivering.size),
                onSend = { text -> store.send(id, text) },
                onStop = { store.act { api.cancel(id) } },
                onSendQueuedNow = { queueId -> store.act { api.sendQueuedNow(id, queueId) } },
                onEditQueued = { queueId, text -> store.act { api.editQueued(id, queueId, text) } },
                onRemoveQueued = { queueId -> store.act { api.removeQueued(id, queueId) } },
                options = remember(conversation, state.events[id]) { sessionOptions(conversation, state.events[id].orEmpty()) },
                commands = remember(state.events[id]) { slashCommands(state.events[id].orEmpty()) },
                onSetMode = { mode -> store.act { api.setConfig(id, ConfigRequest(mode = mode)) } },
                onSetModel = { model -> store.act { api.setConfig(id, ConfigRequest(model = model)) } },
                onSearchFiles = { query -> api.files(id, query) },
                accounts = remember(node, state.nodes, conversation.agent) { node?.let { accountChoices(switchableAccounts(it, conversation.agent), accountsElsewhere(state.nodes.values, it, conversation.agent), it.name) }.orEmpty() },
                accountLocked = conversation.acpSessionId != null,
                onSetAccount = { account ->
                    store.act {
                        when {
                            account.startsWith(SignInPrefix) && node != null -> {
                                val started = api.startLogin(node.id, conversation.agent, loginMethodFor(conversation.agent), account.removePrefix(SignInPrefix))
                                signIn = LoginSession(started.loginId, conversation.agent)
                            }
                            account == DefaultAccountId -> api.patchConversation(id, PatchConversationRequest(useDefaultAccount = true))
                            else -> api.patchConversation(id, PatchConversationRequest(account = account))
                        }
                    }
                },
                onSetSetting = { setting, value -> store.act { api.setConfig(id, ConfigRequest(config = mapOf(setting to JsonPrimitive(value)))) } },
            )
        }

        if (!compact) {
            when (panel) {
                SidePanel.Details -> Inspector(conversation, node)
                SidePanel.Terminal -> key(conversation.id) { TerminalSide(store, state, conversation) }
                SidePanel.Git -> key(conversation.id) { GitSide(store, conversation) }
                null -> Unit
            }

            Rail(panel) { chosen -> panel = if (panel == chosen) null else chosen }
        }
    }

    if (compact && panel != null) {
        BottomSheet(onDismiss = { panel = null }) {
            when (panel) {
                SidePanel.Details -> InspectorContent(conversation, node)
                SidePanel.Terminal -> key(conversation.id) { TerminalPanel(store, state, conversation, Modifier.fillMaxWidth().height(Tokens.Size.TerminalSheet)) }
                SidePanel.Git -> key(conversation.id) { GitPanel(store, conversation, Modifier.fillMaxWidth().height(Tokens.Size.GitSheet)) }
                null -> Unit
            }
        }
    }

    signIn?.let { session -> AccountSignInDialog(store, session, onClose = { signIn = null }) }

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
    onTerminal: () -> Unit,
    onGit: () -> Unit,
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
            TextInput(draft, { draft = it }, label = "Conversation name", placeholder = "Leave empty to use the suggested title", modifier = if (compact) Modifier.weight(1f) else Modifier.width(Tokens.Size.FormColumn), showLabel = false, onSubmit = {
                onRename(draft.trim())
                renaming = false
            })
            MyButton({ onRename(draft.trim()); renaming = false }, text = "Save", size = ButtonSize.Sm, variant = ButtonVariant.Outline)
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
            MyButton(onNode, icon = node?.let(::nodeIcon) ?: MyIcon.Server, description = "Running on ${node?.name ?: "an unknown node"}", muted = true)
            MyButton(onTerminal, icon = MyIcon.Terminal, description = "Terminal", muted = true)
            MyButton(onGit, icon = MyIcon.Branch, description = "Git", muted = true)
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

private enum class SidePanel { Details, Terminal, Git }

@Composable
private fun Rail(open: SidePanel?, onToggle: (SidePanel) -> Unit) {
    val colors = MyTheme.colors

    Row(Modifier.fillMaxHeight()) {
        VerticalHairline()

        Column(
            Modifier.width(Tokens.Size.Rail).fillMaxHeight().background(colors.bgChrome).padding(top = Tokens.Space.S1),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            MyButton({ onToggle(SidePanel.Details) }, icon = MyIcon.Info, description = if (open == SidePanel.Details) "Close details" else "Open details", muted = true, selected = open == SidePanel.Details)
            MyButton({ onToggle(SidePanel.Terminal) }, icon = MyIcon.Terminal, description = if (open == SidePanel.Terminal) "Close terminal" else "Open terminal", muted = true, selected = open == SidePanel.Terminal)
            MyButton({ onToggle(SidePanel.Git) }, icon = MyIcon.Branch, description = if (open == SidePanel.Git) "Close git" else "Open git", muted = true, selected = open == SidePanel.Git)
        }
    }
}

@Composable
private fun TerminalSide(store: Store, state: AppState, conversation: Conversation) {
    Row(Modifier.fillMaxHeight()) {
        VerticalHairline()
        TerminalPanel(store, state, conversation, Modifier.width(Tokens.Size.Terminal).fillMaxHeight())
    }
}

@Composable
private fun GitSide(store: Store, conversation: Conversation) {
    Row(Modifier.fillMaxHeight()) {
        VerticalHairline()
        GitPanel(store, conversation, Modifier.width(Tokens.Size.Git).fillMaxHeight())
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
            "Started" to ago(conversation.createdAt),
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
