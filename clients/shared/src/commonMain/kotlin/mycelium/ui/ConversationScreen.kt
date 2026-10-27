package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.layout.layout
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.semantics.semantics
import mycelium.ProviderDictation
import mycelium.api.Conversation
import mycelium.api.CreateProjectRequest
import mycelium.api.Project
import mycelium.api.ConversationStatus
import mycelium.api.ConfigRequest
import mycelium.api.LoginInfo
import mycelium.api.NodeSummary
import mycelium.api.ModelProvider
import mycelium.api.PatchConversationRequest
import mycelium.state.Annotation
import mycelium.state.Ide
import mycelium.state.OutlinePage
import mycelium.state.DiffTabs
import mycelium.state.DraftAttachment
import mycelium.state.PickedFile
import mycelium.state.attachmentProblem
import mycelium.state.hasOlderEvents
import mycelium.state.mediaTypeOf
import mycelium.state.AppState
import mycelium.state.messageWithAnnotations
import mycelium.state.Store
import mycelium.state.ThreadItem
import mycelium.state.DefaultAccountId
import mycelium.state.SignInPrefix
import mycelium.state.accountChoices
import mycelium.state.accountsElsewhere
import mycelium.state.continueChoices
import mycelium.state.routedLabel
import mycelium.state.routedStatus
import mycelium.state.currentContinueId
import mycelium.state.agentName
import mycelium.state.switchableAccounts
import mycelium.state.isLaptop
import mycelium.state.isChat
import mycelium.state.isSubagent
import mycelium.state.ThreadBuilder
import mycelium.state.reportedModelChoices
import mycelium.state.sessionOptions
import mycelium.state.slashCommands
import mycelium.state.turnIsOpen
import mycelium.state.SubagentSection
import mycelium.state.ago
import mycelium.state.relativeTime
import mycelium.state.subagentSections
import kotlinx.serialization.json.JsonPrimitive
import mycelium.state.loginMethodName
import mycelium.state.statusSectionName

@Composable
fun ConversationScreen(store: Store, state: AppState, id: String, onArchived: () -> Unit, onOpenNodes: () -> Unit = {}, onOpenConversation: (String) -> Unit = {}, onOpenArtefact: (String) -> Unit = {}, panels: Boolean = true, header: HeaderSlots? = null, onManageActions: () -> Unit = {}) {
    val api = store.api
    var fetched by remember(id) { mutableStateOf<Conversation?>(null) }
    val platform = LocalPlatform.current
    var nodeSheetOpen by remember { mutableStateOf(false) }
    var moveOpen by remember { mutableStateOf(false) }
    var viewedArtefact by remember(id) { mutableStateOf<String?>(null) }
    var tab by remember(id) { mutableStateOf(ConversationTab) }
    var dismissedPins by remember(id) { mutableStateOf(store.pinDismissals[id]) }
    var signIn by remember { mutableStateOf<LoginSession?>(null) }
    val compact = LocalTouch.current
    val conversation = state.conversations[id] ?: fetched
    val workspace = conversation?.let { "${it.nodeId}:${it.cwd}" }
    val threadAssistant = conversation?.assistantId?.let { state.assistants[it] }

    LaunchedEffect(threadAssistant?.id) {
        store.act { threadAssistant?.let { store.refreshAssistantActions(it.id); store.refreshActionProposals(it.id) } }
        store.act { store.refreshAssistantChanges() }
    }

    val assistantActions = threadAssistant?.let { held ->
        rememberAssistantActionControls(held, state.assistantActions[held.id].orEmpty(), onManageActions) { action, inputs -> store.act { store.api.runAssistantAction(action.assistantId, action.id, inputs) } }
    }
    var panel by remember(id) {
        val saved = if (compact) null else platform?.loadPreference("$SidePanelPreference:$id")

        mutableStateOf(SidePanel.entries.firstOrNull { it.name == saved && it != SidePanel.Outline })
    }
    var panelWidth by remember(workspace) {
        val saved = workspace?.let { platform?.loadPreference("$SidePanelWidthPreference:$it") } ?: platform?.loadPreference(SidePanelWidthPreference)

        mutableStateOf(saved?.toFloatOrNull()?.dp ?: Tokens.Size.SidePanel)
    }

    var chatForced by remember(id) { mutableStateOf(false) }

    fun openPanel(chosen: SidePanel?) {
        panel = chosen
        chatForced = false

        if (!compact) {
            platform?.savePreference("$SidePanelPreference:$id", chosen?.name.orEmpty())
        }
    }

    var chatWidth by remember { mutableStateOf(Dp.Unspecified) }
    var areaWidth by remember { mutableStateOf(Dp.Unspecified) }
    val demand = LocalPanelDemand.current
    val sidePanels = !compact && panels && conversation?.isChat != true
    val narrow = sidePanels && areaWidth != Dp.Unspecified && panelTakesOver(areaWidth)
    val takeover = narrow && panel != null && !chatForced
    val browserPanel = panel == SidePanel.Outline

    SideEffect {
        demand?.width = if (sidePanels) panelDemand(panelWidth, panel != null, browserPanel) else 0.dp
    }

    DisposableEffect(demand) {
        onDispose { demand?.width = 0.dp }
    }

    LaunchedEffect(narrow) {
        if (!narrow) {
            chatForced = false
        }
    }

    fun resizePanel(width: Dp) {
        val panelMax = if (chatWidth == Dp.Unspecified) Tokens.Size.SidePanelMax else maxOf(Tokens.Size.SidePanelMin, panelWidth + chatWidth - Tokens.Size.ChatMin)
        panelWidth = width.coerceIn(Tokens.Size.SidePanelMin, panelMax)
        platform?.savePreference(SidePanelWidthPreference, panelWidth.value.toString())

        if (workspace != null) {
            platform?.savePreference("$SidePanelWidthPreference:$workspace", panelWidth.value.toString())
        }
    }

    val draft = store.composerDrafts[id]
    var annotations by remember(id) { mutableStateOf(draft.annotations) }

    val attached = rememberAttachmentDrafts(id, draft.attachments)

    LaunchedEffect(id) {
        snapshotFlow { annotations to attached.drafts }.collect { (current, files) ->
            store.composerDrafts.update(id) { copy(annotations = current, attachments = files) }
        }
    }

    fun send(text: String) {
        store.send(id, messageWithAnnotations(annotations, store.withWalkthroughNotes(id, text)), attached.drafts.map { it.file })
        annotations = emptyList()
        attached.clear()
    }

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

    val groupId = conversation.groupId

    LaunchedEffect(groupId) {
        if (groupId != null && state.groups[groupId] == null) {
            store.load { store.refreshGroups() }
        }
    }

    val groupName = groupId?.let { state.groups[it]?.name }
    val chat = conversation.isChat
    var outlinePage by remember(id) { mutableStateOf<OutlinePage?>(null) }
    val showOutline: (OutlinePage) -> Unit = { page ->
        if (!compact && !chat && panels) {
            outlinePage = page
            openPanel(SidePanel.Outline)
        } else {
            platform?.openUrl(page.url)
        }
    }
    val outlineOnEntry = remember(id) { state.outlinePages[id] }
    val liveOutline = state.outlinePages[id]

    LaunchedEffect(liveOutline) {
        if (liveOutline != null && liveOutline != outlineOnEntry) {
            showOutline(liveOutline)
        }
    }

    var providers by remember { mutableStateOf(emptyList<ModelProvider>()) }

    LaunchedEffect(Unit) {
        store.act { providers = api.modelProviders() }
    }

    val scope = rememberCoroutineScope()
    val speechProvided = providers.any { it.speechModel != null }
    val dictation = remember(platform, state.experiments.voice, speechProvided) {
        when {
            !state.experiments.voice -> null
            else -> platform?.dictation() ?: platform?.audioRecorder()?.takeIf { speechProvided }?.let { ProviderDictation(it, api::transcribe, scope) }
        }
    }

    val node = if (chat) null else state.nodes[conversation.nodeId]
    val ides = LocalIdes.current
    val ideTarget = if (node != null && !chat && ides != null) IdeTarget(node, conversation.cwd) else null
    val ideMain = node?.let { ides?.main(it) }
    val ideControls = if (ides != null && node != null && ideMain != null && !chat) {
        IdeControls(ideMain, ides.others(node), ides.where(node), { ide -> ides.open(node, ide, conversation.cwd) }, ides.openSettings)
    } else {
        null
    }
    val subagentParent = conversation.parentId?.takeIf { conversation.isSubagent }?.let { state.conversations[it] }
    val sideKind = { kind: String -> if (chat) "side" else kind }
    val threadBuilder = remember(id) { ThreadBuilder() }
    val thread = remember(state.events[id]) { threadBuilder.build(state.events[id].orEmpty()) }
    val agentWorking = conversation.agentActive || (conversation.status == ConversationStatus.Working && turnIsOpen(state.events[id].orEmpty()))
    val sending = state.sending[id].orEmpty()
    val deliveredPrompts = state.events[id].orEmpty().filter { it.kind == mycelium.api.EventKinds.UserMessage }.mapNotNull { event ->
        val payload = event.payload as? kotlinx.serialization.json.JsonObject
        val promptId = payload?.get("prompt_id") as? JsonPrimitive

        promptId?.content
    }.toSet()
    val queue = state.queues[id].orEmpty().filterNot { it.id in deliveredPrompts }
    val starting = thread.none { it is ThreadItem.UserMessage } && !state.hasOlderEvents(id)
    val initialQueued = if (starting) queue.minByOrNull { it.position } else null
    val goesStraight = conversation.status in setOf(ConversationStatus.Review, ConversationStatus.Done) && queue.isEmpty()
    val delivering = sending.take(if (goesStraight || (starting && initialQueued == null)) 1 else 0)
    val initialMessage = initialQueued?.let { ThreadItem.UserMessage("queued:${it.id}", it.text, files = it.attachments) }

    val annotate = remember(id) {
        AnnotateActions(
            onAttach = { added -> annotations = annotations + added },
            onAsk = { quote, question -> store.act { onOpenConversation(api.startSideChat(id, sideKind("btw"), messageWithAnnotations(listOf(Annotation(quote, "")), question)).id) } },
        )
    }

    val generatedActions = GeneratedUiActions(
        onSendMessage = { text -> store.send(id, text) },
        onSideChat = { text -> store.act { onOpenConversation(api.startSideChat(id, sideKind("btw"), text).id) } },
        canOpenFiles = !chat,
    )

    val outerOpener = LocalArtifactOpener.current
    val artefactOpener = outerOpener?.let { outer ->
        ArtifactOpener(outer.artifacts) { artifactId ->
            if (panels && !chat && conversationArtefacts(state, id).any { it.id == artifactId }) {
                viewedArtefact = artifactId
                openPanel(SidePanel.Artefacts)
            } else {
                outer.open(artifactId)
            }
        }
    }

    val agentOpener: (String) -> (() -> Unit)? = { agentId -> state.conversations.values.firstOrNull { it.parentId == id && it.agentId == agentId }?.let { agentConversation -> { onOpenConversation(agentConversation.id) } } }

    CompositionLocalProvider(LocalAnnotate provides annotate, LocalGeneratedUiActions provides generatedActions, LocalAttachmentBytes provides store::attachmentBytes, LocalArtifactOpener provides artefactOpener, LocalAgentOpener provides agentOpener, LocalOutlineOpener provides showOutline, LocalOutlinePage provides (outlinePage ?: liveOutline), LocalIdeTarget provides ideTarget) {
        val walkthrough = state.walkthroughs[id]?.takeIf { !it.dismissed && !chat }

        val density = LocalDensity.current

        val panelContent: @Composable () -> Unit = {
            when (panel) {
                SidePanel.Details -> Inspector(conversation, node, groupName)
                SidePanel.Subagents -> SubagentsPanel(subagentSections(state.conversations.values, id), id, subagentParent, onOpenConversation, store::closeSession)
                SidePanel.Terminal -> key(conversation.id) { TerminalPanel(store, state, conversation, Modifier.fillMaxSize(), onExited = { openPanel(null) }) }
                SidePanel.Git -> key(conversation.id) { GitPanel(store, conversation, Modifier.fillMaxSize(), onOpenDiff = { store.openDiffTab(conversation.id, it) }) }
                SidePanel.Artefacts -> platform?.let { ArtefactsPanel(store, state, it, id, viewedArtefact, { viewedArtefact = it }, onOpenArtefact, Modifier.fillMaxSize()) }
                SidePanel.Outline -> outlinePage?.let { page -> platform?.let { BrowserPanel(it, page.url, page.title, onClose = { openPanel(null) }) } }
                null -> Unit
            }
        }

        Row(Modifier.fillMaxSize().onSizeChanged { areaWidth = with(density) { it.width.toDp() } }) {
            Column(
                Modifier
                    .then(if (takeover) Modifier.width(0.dp) else Modifier.weight(1f))
                    .fillMaxHeight()
                    .layout { measurable, constraints ->
                        if (takeover) {
                            layout(0, 0) {}
                        } else {
                            val placeable = measurable.measure(constraints)

                            layout(placeable.width, placeable.height) { placeable.place(0, 0) }
                        }
                    }
                    .onSizeChanged { chatWidth = with(density) { it.width.toDp() } },
            ) {
                ConversationHeader(
                    slots = header,
                    conversation = conversation,
                    node = node,
                    chat = chat,
                    compact = compact,
                    onNode = { nodeSheetOpen = true },
                    moveSheet = { dismiss -> MoveSheet(store, state, conversation, dismiss, onOpenConversation) },
                    ide = ideControls,
                    onDetails = { openPanel(SidePanel.Details) },
                    onTerminal = { openPanel(SidePanel.Terminal) },
                    onGit = { openPanel(SidePanel.Git) },
                    project = state.projects[conversation.projectId],
                    projects = state.projects.values.filter { it.nodeId == conversation.nodeId }.sortedBy { it.name },
                    nodes = state.nodes,
                    onProject = { projectId -> store.setConversationProject(conversation, projectId) },
                    onSetUp = { store.act { store.createProject(CreateProjectRequest(conversation.nodeId, path = conversation.cwd)) } },
                    onRename = { title -> store.rename(conversation, title) },
                    onPin = { store.pin(conversation, conversation.pinnedAt == null) },
                    onCloseSession = { store.closeSession(conversation) },
                    onArchive = {
                        store.act {
                            api.patchConversation(id, PatchConversationRequest(archived = conversation.archivedAt == null))
                            onArchived()
                        }
                    },
                    onRetry = { store.act { api.retryConversation(id) } },
                    onDismissFailure = { store.dismissFailure(conversation) },
                    dismissedWalkthrough = !chat && state.walkthroughs[id]?.dismissed == true,
                    onShowWalkthrough = { store.setWalkthroughDismissed(id, false) },
                    onDelete = {
                        store.act {
                            api.deleteConversation(id)
                            onArchived()
                        }
                    },
                )

                val diffs = store.diffTabs.collectAsState().value[id] ?: DiffTabs()
                val activeDiff = diffs.tabs.firstOrNull { it.key == diffs.active }

                if (!compact && !chat && (diffs.tabs.isNotEmpty() || walkthrough != null)) {
                    val shown = if (tab == WalkthroughTab && walkthrough == null) ConversationTab else tab

                    MainTabStrip(
                        tabs = listOf(MainTab(ConversationTab, ConversationTab)) +
                            listOfNotNull(walkthrough?.let { MainTab(WalkthroughTab, WalkthroughTab, "Dismiss walkthrough") }) +
                            diffs.tabs.map { MainTab(it.key, it.title, it.closeLabel) },
                        selected = activeDiff?.key ?: shown,
                        onSelect = { key ->
                            store.focusDiffTab(id, key.takeIf { it != ConversationTab && it != WalkthroughTab })

                            if (key == ConversationTab || key == WalkthroughTab) {
                                tab = key
                            }
                        },
                        onClose = { key -> if (key == WalkthroughTab) store.setWalkthroughDismissed(id, true) else store.closeDiffTab(id, key) },
                        groupLabel = "Main view tabs",
                        modifier = Modifier.padding(horizontal = Tokens.Space.S3),
                    )
                    Hairline()

                    if (activeDiff != null) {
                        key(activeDiff.key) {
                            if (activeDiff.hash != null) CommitTabView(store, conversation, activeDiff, Modifier.weight(1f)) else ChangesTabView(store, conversation, activeDiff, Modifier.weight(1f))
                        }

                        return@Column
                    }

                    if (shown == WalkthroughTab && walkthrough != null) {
                        WalkthroughView(store, id, walkthrough, Modifier.weight(1f))

                        return@Column
                    }
                }

                if (walkthrough != null && compact) {
                    TabRow(listOf(ConversationTab, WalkthroughTab), tab, { tab = it }, groupLabel = "Conversation sections", modifier = Modifier.padding(horizontal = Tokens.Space.S3))
                    Hairline()

                    if (tab == WalkthroughTab) {
                        WalkthroughView(store, id, walkthrough, Modifier.weight(1f))

                        return@Column
                    }
                }

                val responses = remember(id) {
                    ResponseActions(
                        load = { requestId -> api.response(id, requestId) },
                        filter = { requestId, filter -> api.filterResponse(id, requestId, filter) },
                        send = { requestId, send -> store.act { api.sendResponse(id, requestId, send) } },
                    )
                }

                val limitActions = LimitActions(
                    resumeAt = conversation.resumeAt,
                    onResume = { store.act { api.scheduleResume(id) } },
                    onRetryNow = { store.act { api.retryConversation(id) } },
                    onCancelResume = { store.act { api.cancelResume(id) } },
                )

                CompositionLocalProvider(LocalResponseActions provides responses, LocalPausedWorkflow provides PausedWorkflow(conversation.agent, limitActions)) {
                    ThreadView(
                        items = thread + listOfNotNull(initialMessage) + delivering.map { ThreadItem.UserMessage("sending", it) },
                        hasOlder = state.hasOlderEvents(id),
                        loadingOlder = id in state.loadingOlderEvents,
                        onLoadOlder = { store.act { store.loadOlder(id) } },
                        onAnswer = { requestId, optionId -> store.act { api.answerPermission(id, requestId, optionId) } },
                        onStopWorkflow = { store.act { api.cancel(id) } },
                        modifier = Modifier.weight(1f),
                        onSuggest = ::send,
                        onRetry = if (conversation.status == ConversationStatus.Failed) ({ store.act { api.retryConversation(id) } }) else null,
                        limitActions = if (conversation.status == ConversationStatus.Failed) limitActions else null,
                        agent = conversation.agent,
                        dismissedPins = dismissedPins,
                        onUnpin = { key ->
                            store.pinDismissals.dismiss(id, key)
                            dismissedPins = store.pinDismissals[id]
                        },
                    )
                }

                state.schedules.values.filter { it.proposed && it.conversationId == id }.forEach { ProposedScheduleCard(store, it, state) }
                state.actionProposals.values.filter { it.conversationId == id }.sortedBy { it.createdAt }.forEach { ProposedActionCard(store, state, it) }
                state.assistantChanges.values.filter { it.conversationId == id }.sortedBy { it.createdAt }.forEach { AssistantChangeCard(store, state, it, onOpenSettings = onManageActions) }

                val parentId = conversation.parentId

                if (conversation.isSubagent && parentId != null) {
                    Row(Modifier.fillMaxWidth().padding(Tokens.Space.S3), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3), verticalAlignment = Alignment.CenterVertically) {
                        Label("A subagent's conversation is read-only.", Modifier.weight(1f), color = MyTheme.colors.textMuted)
                        ConversationUsageBar(api, id, state.events[id].orEmpty().count { it.kind == mycelium.api.EventKinds.TurnEnd })
                        MyButton({ onOpenConversation(parentId) }, text = "Open the conversation it belongs to", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
                    }
                } else {
                    Composer(
                        conversation = conversation,
                        usage = { ConversationUsageBar(api, id, state.events[id].orEmpty().count { it.kind == mycelium.api.EventKinds.TurnEnd }) },
                        working = agentWorking,
                        queue = queue.filter { it.id != initialQueued?.id },
                        sending = sending.drop(delivering.size),
                        onSend = ::send,
                        annotations = annotations,
                        onRemoveAnnotation = { removed -> annotations = annotations - removed },
                        attachments = attached.controls(),
                        assistantActions = assistantActions,
                        dictation = dictation,
                        initialText = TextFieldValue(draft.text, TextRange(draft.cursor)),
                        onTextChange = { typed -> store.composerDrafts.update(id) { copy(text = typed.text, cursor = typed.selection.end) } },
                        modelPicker = if (chat) ModelPickerControls(
                            providers,
                            conversation.providerId,
                            conversation.model,
                            recent = recentModels(state.conversations.values, providers),
                            onFavourite = { picked, favouriteModel, on -> store.setFavouriteModel({ change -> providers = change(providers) }, picked, favouriteModel, on) },
                        ) { chosen, model -> store.act { api.patchConversation(id, PatchConversationRequest(providerId = chosen.id, model = model)) } } else null,
                        onStop = { store.act { api.cancel(id) } },
                        onSendQueuedNow = { queueId -> store.act { api.sendQueuedNow(id, queueId) } },
                        onSendAllQueuedNow = { store.act { api.sendAllQueuedNow(id) } },
                        onEditQueued = { queueId, text -> store.act { api.editQueued(id, queueId, text) } },
                        onRemoveQueued = { queueId -> store.act { api.removeQueued(id, queueId) } },
                        options = remember(conversation, state.events[id], state.sessionUpdates[id], state.nodes[conversation.nodeId]) { sessionOptions(conversation, state.events[id].orEmpty(), state.sessionUpdates[id].orEmpty(), reportedModelChoices(state.nodes[conversation.nodeId], conversation.agent, conversation.account)) },
                        commands = remember(state.events[id], state.sessionUpdates[id]) { slashCommands(state.events[id].orEmpty(), state.sessionUpdates[id].orEmpty()) },
                        onSetMode = { mode -> store.act { api.setConfig(id, ConfigRequest(mode = mode)) } },
                        onSetModel = { model -> store.act { api.setConfig(id, ConfigRequest(model = model)) } },
                        onSearchFiles = { query -> api.files(id, query) },
                        accounts = remember(node, state.nodes, conversation.agent) { node?.let { accountChoices(switchableAccounts(it, conversation.agent), accountsElsewhere(state.nodes.values, it, conversation.agent), it.name) }.orEmpty() },
                        accountLocked = conversation.acpSessionId != null,
                        continueOn = run {
                            val agentSettings = rememberAgentSettings(store)
                            val choices = remember(conversation, state.nodes, providers, agentSettings) { continueChoices(conversation, state.nodes.values, providers, agentSettings.anyModel) }
                            val login = node?.logins?.firstOrNull { it.agent == conversation.agent && it.account == conversation.account }

                            ContinueControls(choices, currentContinueId(conversation), { choice -> store.act { store.continueOn(id, choice) } }, routedLabel(conversation, providers), routedStatus(conversation, login))
                        },
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
                        onSideChat = { kind, text -> store.act { onOpenConversation(api.startSideChat(id, sideKind(kind), text.ifEmpty { null }).id) } },
                        onTeachMe = { on -> store.act { api.patchConversation(id, PatchConversationRequest(teachMe = on)) } },
                        goal = if (chat) null else GoalControls(
                            goal = state.goals[id],
                            onSet = { text, limits -> store.act { store.setGoal(id, text, limits) } },
                            onPause = { store.act { store.pauseGoal(id, true) } },
                            onResume = { store.act { store.pauseGoal(id, false) } },
                            onClear = { store.act { store.clearGoal(id) } },
                            onSendAgain = { store.act { store.sendGoalAgain(id) } },
                            agent = conversation.agent,
                        ),
                    )
                }
            }

            if (!compact && !chat && panels) {
                if (takeover) {
                    Column(Modifier.weight(1f).fillMaxHeight().background(MyTheme.colors.bgPanel)) {
                        Bar {
                            OpenListButton()
                            MyButton({ chatForced = true }, text = "Back to conversation", icon = MyIcon.ChevronLeft, muted = true)
                        }

                        Box(Modifier.weight(1f).fillMaxWidth()) {
                            panelContent()
                        }
                    }
                } else if (panel != null && !narrow) {
                    ResizeHandle("Resize the side panel") { moved -> resizePanel(panelWidth - moved) }

                    Box(Modifier.width(shownPanelWidth(panelWidth, areaWidth, browserPanel)).fillMaxHeight()) {
                        panelContent()
                    }
                }

                Rail(if (narrow && chatForced) null else panel, outlinePage != null) { chosen ->
                    if (narrow) {
                        if (panel == chosen && !chatForced) {
                            chatForced = true
                        } else {
                            openPanel(chosen)
                        }
                    } else {
                        openPanel(if (panel == chosen) null else chosen)
                    }
                }
            }
        }
    }

    if (compact && !chat && panels && panel != null) {
        BottomSheet(onDismiss = { openPanel(null) }) {
            when (panel) {
                SidePanel.Details -> InspectorContent(conversation, node, groupName)
                SidePanel.Subagents -> SubagentsList(subagentSections(state.conversations.values, id), id, subagentParent, onOpenConversation, store::closeSession)
                SidePanel.Terminal -> key(conversation.id) { TerminalPanel(store, state, conversation, Modifier.fillMaxWidth().height(Tokens.Size.TerminalSheet), onExited = { openPanel(null) }) }
                SidePanel.Git -> key(conversation.id) { GitPanel(store, conversation, Modifier.fillMaxWidth().height(Tokens.Size.GitSheet)) }
                SidePanel.Artefacts -> platform?.let { ArtefactsPanel(store, state, it, id, viewedArtefact, { viewedArtefact = it }, { openPanel(null); onOpenArtefact(it) }, Modifier.fillMaxWidth().height(Tokens.Size.GitSheet)) }
                SidePanel.Outline, null -> Unit
            }
        }
    }

    signIn?.let { session -> AccountSignInDialog(store, session, onClose = { signIn = null }) }

    if (compact && nodeSheetOpen) {
        NodeSheet(node, onDismiss = { nodeSheetOpen = false }, onOpenNodes = { nodeSheetOpen = false; onOpenNodes() }, onMove = { nodeSheetOpen = false; moveOpen = true })
    }

    if (compact && moveOpen) {
        MoveSheet(store, state, conversation, { moveOpen = false }, onOpenConversation)
    }
}

@Composable
private fun ConversationHeader(
    slots: HeaderSlots?,
    conversation: Conversation,
    node: NodeSummary?,
    chat: Boolean,
    compact: Boolean,
    onNode: () -> Unit,
    moveSheet: @Composable (onDismiss: () -> Unit) -> Unit,
    ide: IdeControls?,
    onDetails: () -> Unit,
    onTerminal: () -> Unit,
    onGit: () -> Unit,
    project: Project?,
    projects: List<Project>,
    nodes: Map<String, NodeSummary>,
    onProject: (String?) -> Unit,
    onSetUp: () -> Unit,
    onRename: (String) -> Unit,
    onPin: () -> Unit,
    onCloseSession: () -> Unit,
    onArchive: () -> Unit,
    onRetry: () -> Unit,
    onDismissFailure: () -> Unit,
    dismissedWalkthrough: Boolean,
    onShowWalkthrough: () -> Unit,
    onDelete: () -> Unit,
) {
    val colors = MyTheme.colors
    var confirmingDelete by remember(conversation.id) { mutableStateOf(false) }
    var renaming by remember(conversation.id) { mutableStateOf(false) }
    var draft by remember(conversation.title) { mutableStateOf(conversation.title) }
    var menuOpen by remember { mutableStateOf(false) }
    var nodeMenuOpen by remember { mutableStateOf(false) }
    var moveOpen by remember { mutableStateOf(false) }
    var projectMenuOpen by remember { mutableStateOf(false) }
    val archived = conversation.archivedAt != null

    Bar(startPadding = if (compact) Tokens.Space.S1 else Tokens.Space.S3) {
        OpenListButton()

        if (slots != null) {
            slots.title(this)
        } else if (renaming) {
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

        if (compact && chat) {
            Box(Modifier.weight(0.001f))
        } else if (compact) {
            MyButton(onNode, icon = node?.let(::nodeIcon) ?: MyIcon.Server, description = "Running on ${node?.name ?: "an unknown node"}", muted = true)
            MyButton(onTerminal, icon = MyIcon.Terminal, description = "Terminal", muted = true)
            MyButton(onGit, icon = MyIcon.Branch, description = "Git", muted = true)

            MyButton(onDetails, icon = MyIcon.Info, description = "Details", muted = true)
        } else {
            if (chat) {
                Label(conversation.model.orEmpty(), style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
            } else {
                ProjectBreadcrumb(project, projects, nodes, onProject, setUpFolder = conversation.cwd.takeIf { project == null }, onSetUp = onSetUp)
            }

            if (node != null) {
                Box {
                    MyButton({ nodeMenuOpen = true }, text = node.name, icon = nodeIcon(node), trailingIcon = MyIcon.ChevronDown, description = "Running on ${node.name}, open the node menu", size = ButtonSize.Sm, muted = true)

                    if (nodeMenuOpen) {
                        MenuPopup(onDismiss = { nodeMenuOpen = false }) {
                            MenuItem("Move", onClick = { nodeMenuOpen = false; moveOpen = true }, leading = MyIcon.Move)
                        }
                    }

                    if (moveOpen) {
                        moveSheet { moveOpen = false }
                    }
                }
            }

            if (archived) {
                Tag("Archived")
            } else if (conversation.status == ConversationStatus.Failed) {
                MyButton(onRetry, text = "Retry", size = ButtonSize.Sm, variant = ButtonVariant.Outline, description = "Retry ${conversation.title}")
                MyButton(onDismissFailure, text = "Dismiss", size = ButtonSize.Sm, muted = true, description = "Dismiss the failure of ${conversation.title}")
            }

            Box(Modifier.weight(1f))
        }

        if (ide != null && !compact) {
            SplitButton(
                "Open in ${ide.main.label}",
                { ide.open(ide.main) },
                ide.others.map { SplitAlternative("Open in ${it.label}", { ide.open(it) }) } + SplitAlternative("IDE settings", ide.onSettings, separated = true),
                variant = ButtonVariant.Outline,
                size = ButtonSize.Sm,
                menuLabel = ide.where,
            )
        }

        Box {
            if (!compact || slots != null) {
                MyButton({ menuOpen = true }, icon = MyIcon.MoreVertical, description = if (slots != null) "More for ${conversation.title}" else "More", muted = true)
            }

            if (menuOpen) {
                MenuPopup(onDismiss = { menuOpen = false }) {
                    if (slots != null) {
                        slots.menu { menuOpen = false }
                    } else {
                    if (compact) {
                        MenuItem("Rename", onClick = { menuOpen = false; renaming = true }, leading = MyIcon.Pencil)

                        if (!chat) {
                            MenuItem("Project", onClick = { menuOpen = false; projectMenuOpen = true }, hint = project?.name ?: "None", leading = MyIcon.Folder)
                        }
                    }

                    MenuItem(if (conversation.pinnedAt == null) "Pin" else "Unpin", onClick = { menuOpen = false; onPin() }, leading = MyIcon.Pin)

                    if (dismissedWalkthrough) {
                        MenuItem("Show walkthrough", onClick = { menuOpen = false; onShowWalkthrough() }, leading = MyIcon.Code)
                    }

                    if (!chat && !archived) {
                        MenuItem("Close session", onClick = { menuOpen = false; onCloseSession() }, leading = MyIcon.Stop)
                    }

                    MenuItem(if (archived) "Restore" else "Archive", onClick = { menuOpen = false; onArchive() }, leading = MyIcon.Archive)

                    if (archived) {
                        MenuItem("Delete", onClick = { menuOpen = false; confirmingDelete = true })
                    }
                    }
                }
            }
        }
    }

    slots?.notices?.invoke()

    if (projectMenuOpen) {
        ProjectMenu(projects, nodes, project?.id, "Project", onDismiss = { projectMenuOpen = false }, setUpFolder = conversation.cwd.takeIf { project == null }, onSetUp = { projectMenuOpen = false; onSetUp() }) {
            projectMenuOpen = false
            onProject(it)
        }
    }

    if (confirmingDelete) {
        DeleteConversationDialog(conversation.title, onDismiss = { confirmingDelete = false }, onConfirm = { confirmingDelete = false; onDelete() })
    }
}

class IdeControls(val main: Ide, val others: List<Ide>, val where: String, val open: (Ide) -> Unit, val onSettings: () -> Unit)

class HeaderSlots(val title: @Composable RowScope.() -> Unit, val menu: @Composable (close: () -> Unit) -> Unit, val notices: @Composable () -> Unit)

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

private const val ConversationTab = "Conversation"
private const val WalkthroughTab = "Walkthrough"

private enum class SidePanel { Details, Subagents, Terminal, Git, Artefacts, Outline }

@Composable
private fun Rail(open: SidePanel?, outlineOpen: Boolean, onToggle: (SidePanel) -> Unit) {
    val colors = MyTheme.colors

    Row(Modifier.fillMaxHeight()) {
        VerticalHairline()

        Column(
            Modifier.width(Tokens.Size.Rail).fillMaxHeight().background(colors.bgChrome).padding(top = Tokens.Space.S1),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            MyButton({ onToggle(SidePanel.Details) }, icon = MyIcon.Info, description = if (open == SidePanel.Details) "Close details" else "Open details", muted = true, selected = open == SidePanel.Details)
            MyButton({ onToggle(SidePanel.Subagents) }, icon = MyIcon.Agent, description = if (open == SidePanel.Subagents) "Close subagents" else "Open subagents", muted = true, selected = open == SidePanel.Subagents)

            MyButton({ onToggle(SidePanel.Terminal) }, icon = MyIcon.Terminal, description = if (open == SidePanel.Terminal) "Close terminal" else "Open terminal", muted = true, selected = open == SidePanel.Terminal)
            MyButton({ onToggle(SidePanel.Git) }, icon = MyIcon.Branch, description = if (open == SidePanel.Git) "Close git" else "Open git", muted = true, selected = open == SidePanel.Git)
            MyButton({ onToggle(SidePanel.Artefacts) }, icon = MyIcon.Layers, description = if (open == SidePanel.Artefacts) "Close artefacts" else "Open artefacts", muted = true, selected = open == SidePanel.Artefacts)

            if (outlineOpen) {
                MyButton({ onToggle(SidePanel.Outline) }, icon = MyIcon.Globe, description = if (open == SidePanel.Outline) "Close Outline page" else "Open Outline page", muted = true, selected = open == SidePanel.Outline)
            }
        }
    }
}

@Composable
private fun Inspector(conversation: Conversation, node: NodeSummary?, groupName: String?) {
    val colors = MyTheme.colors

    Column(Modifier.fillMaxSize().background(colors.bgPanel).semantics { contentDescription = "Conversation details" }) {
        Bar {
            BarTitle("Details")
        }

        Column(Modifier.weight(1f).verticalScrollWithBar().padding(top = Tokens.Space.S2, bottom = Tokens.Space.S4)) {
            InspectorContent(conversation, node, groupName)
        }
    }
}

@Composable
private fun SubagentsPanel(sections: List<SubagentSection>, currentId: String, parent: Conversation?, onOpen: (String) -> Unit, onStop: (Conversation) -> Unit) {
    Column(Modifier.fillMaxSize().background(MyTheme.colors.bgPanel).semantics { contentDescription = "Subagents" }) {
        Bar {
            BarTitle("Subagents")
        }

        Column(Modifier.weight(1f).verticalScrollWithBar().padding(vertical = Tokens.Space.S2)) {
            SubagentsList(sections, currentId, parent, onOpen, onStop)
        }
    }
}

@Composable
private fun SubagentsList(sections: List<SubagentSection>, currentId: String, parent: Conversation?, onOpen: (String) -> Unit, onStop: (Conversation) -> Unit) {
    val colors = MyTheme.colors

    if (parent != null) {
        BackButton(parent.title, { onOpen(parent.id) }, Modifier.padding(horizontal = Tokens.Space.S2), showText = true)
    }

    if (sections.isEmpty()) {
        Label("This conversation has no subagents.", Modifier.padding(horizontal = Tokens.Space.S3), color = colors.textMuted)
    }

    sections.forEachIndexed { index, section ->
        if (index > 0) {
            Hairline(Modifier.padding(vertical = Tokens.Space.S1_5))
        }

        if (sections.size > 1) {
            Label(section.owner.title, Modifier.padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S1).semantics { heading() }, Tokens.Type.MetaStrong, colors.textMuted, maxLines = 1)
        }

        section.subagents.forEach { subagent ->
            val status = statusSectionName(subagent.status)
            val since = relativeTime(subagent.createdAt)

            val menu = rememberRowMenuState()
            val running = subagent.status == ConversationStatus.Working || subagent.status == ConversationStatus.Input

            Row(
                Modifier
                    .fillMaxWidth()
                    .rowContextMenu(menu)
                    .pressable({ onOpen(subagent.id) }, selected = subagent.id == currentId, shape = BlockShape, description = "${subagent.title}, $status, started ${ago(subagent.createdAt)}")
                    .padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S1_5),
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Icon(iconOf(subagent.status), colors.textMuted, Tokens.Size.IconSm)
                Label(subagent.title, Modifier.weight(1f), maxLines = 1)
                Label(status, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
                Label(since, style = Tokens.Type.Meta, color = colors.textFaint, maxLines = 1)

                RowMenu(subagent.title, menu) { close ->
                    MenuItem("Open", onClick = { close(); onOpen(subagent.id) }, leading = MyIcon.Agent)

                    if (running) {
                        MenuItem("Stop", onClick = { close(); onStop(subagent) }, leading = MyIcon.Stop)
                    }
                }
            }
        }
    }
}

@Composable
private fun InspectorContent(conversation: Conversation, node: NodeSummary?, groupName: String?) {
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
            "Group" to (groupName ?: "None"),
            "Status" to statusSectionName(conversation.status),
            "Mode" to (conversation.mode ?: "Default"),
            "Model" to (conversation.model ?: "Not set"),
            "Started" to timestamp(conversation.createdAt, ago = true),
        ),
    )
}

@Composable
private fun NodeSheet(node: NodeSummary?, onDismiss: () -> Unit, onOpenNodes: () -> Unit, onMove: (() -> Unit)?) {
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

        if (onMove != null) {
            MenuItem("Move", onClick = onMove, leading = MyIcon.Move)
        }

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

private const val SidePanelPreference = "side-panel"

private const val SidePanelWidthPreference = "side-panel-width"
