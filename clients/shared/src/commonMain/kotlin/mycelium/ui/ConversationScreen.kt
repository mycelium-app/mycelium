package mycelium.ui

import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.withStyle
import androidx.compose.foundation.text.BasicText
import androidx.compose.ui.text.style.TextOverflow
import mycelium.api.ModelChoice
import mycelium.api.Routing
import mycelium.state.appsLinkVisible
import mycelium.state.ContinueModelPrefix
import mycelium.state.RoutingPrefix
import mycelium.state.anyAccountId
import mycelium.state.currentTodos
import mycelium.state.todosAreShown
import mycelium.state.waitsToBeOpened
import mycelium.state.conversationModelLabel
import mycelium.state.modelOptions
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.gestures.Orientation
import androidx.compose.foundation.layout.height
import androidx.compose.ui.draw.clipToBounds
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
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.semantics.semantics
import mycelium.ProviderDictation
import mycelium.state.VoicePhase
import mycelium.api.Conversation
import mycelium.api.Workflow
import mycelium.api.WorkflowControlRequest
import mycelium.state.WorkflowStepStatuses
import mycelium.api.WorkflowStatuses
import mycelium.api.Watch
import mycelium.api.CreateProjectRequest
import mycelium.api.Project
import mycelium.api.ApiException
import mycelium.api.ConversationStatus
import mycelium.api.EnvironmentState
import mycelium.api.ConfigRequest
import mycelium.api.LoginInfo
import mycelium.api.NodeSummary
import mycelium.api.ModelProvider
import mycelium.api.PatchConversationRequest
import mycelium.api.PatchQueueRequest
import mycelium.state.forgeReferences
import mycelium.state.Annotation
import mycelium.state.QueueEditing
import mycelium.state.ComposerDraft
import mycelium.state.Ide
import mycelium.state.OutlinePage
import mycelium.state.outlineIntegrations
import mycelium.state.DiffTabs
import mycelium.state.DraftAttachment
import mycelium.state.PickedFile
import mycelium.state.attachmentProblem
import mycelium.state.hasOlderEvents
import mycelium.state.mediaTypeOf
import mycelium.state.AppState
import mycelium.state.messageWithAnnotations
import mycelium.state.pathWithoutLine
import mycelium.state.parentFolder
import mycelium.state.Store
import mycelium.state.hasSpeechToText
import mycelium.state.ThreadItem
import mycelium.state.continueAccountId
import mycelium.state.pickerAccounts
import mycelium.state.signInAccounts
import mycelium.state.ContinueChatPrefix
import mycelium.state.routedStatus
import mycelium.state.currentContinueId
import mycelium.state.agentName
import mycelium.state.ContinueAccountPrefix
import mycelium.state.DefaultAccountId
import mycelium.state.SignInPrefix
import mycelium.state.isLaptop
import mycelium.state.sessionGrants
import mycelium.state.isChat
import mycelium.state.isSubagent
import mycelium.state.ThreadBuilder
import mycelium.state.reportedModelChoices
import mycelium.state.sessionOptions
import mycelium.state.slashCommands
import mycelium.state.turnIsOpen
import mycelium.state.HistoryEntry
import mycelium.state.SubagentSection
import mycelium.state.ago
import mycelium.state.relativeTime
import mycelium.state.subagentSections
import kotlinx.serialization.json.JsonPrimitive
import mycelium.state.loginMethodName
import mycelium.state.statusSectionName

@Composable
fun ConversationScreen(store: Store, state: AppState, id: String, onArchived: () -> Unit, onOpenNodes: () -> Unit = {}, onOpenNode: (String) -> Unit = {}, onOpenConversation: (String) -> Unit = {}, onOpenArtefact: (String) -> Unit = {}, panels: Boolean = true, header: HeaderSlots? = null, onManageActions: () -> Unit = {}) {
    val api = store.api
    var fetched by remember(id) { mutableStateOf<Conversation?>(null) }
    var watches by remember(id) { mutableStateOf<List<Watch>>(emptyList()) }

    LaunchedEffect(id, state.watchesRevision[id]) {
        store.act { watches = api.watches(id) }
    }
    val platform = LocalPlatform.current
    var nodeSheetOpen by remember { mutableStateOf(false) }
    var moveOpen by remember { mutableStateOf(false) }
    var moveTo by remember { mutableStateOf<String?>(null) }
    var phoneEnvironment by remember(id) { mutableStateOf(false) }
    var viewedArtefact by remember(id) { mutableStateOf<String?>(null) }
    var viewedApp by remember(id) { mutableStateOf<String?>(null) }
    var tab by remember(id) { mutableStateOf(ConversationTab) }
    var dismissedPins by remember(id) { mutableStateOf(store.pinDismissals[id]) }
    val compact = LocalTouch.current
    var signIn by remember { mutableStateOf<LoginSession?>(null) }
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

        mutableStateOf(SidePanel.entries.firstOrNull { it.name == saved && it != SidePanel.Outline && it != SidePanel.Apps && (it != SidePanel.PullRequest || conversation?.pullRequestNumber != null) && (conversation?.isChat != true || it == SidePanel.Environment) })
    }
    var panelWidth by remember(workspace) {
        val saved = workspace?.let { platform?.loadPreference("$SidePanelWidthPreference:$it") } ?: platform?.loadPreference(SidePanelWidthPreference)

        mutableStateOf(saved?.toFloatOrNull()?.dp ?: Tokens.Size.SidePanel)
    }

    var chatForced by remember(id) { mutableStateOf(false) }
    var terminalShown by remember(id) { mutableStateOf(false) }
    var terminalMounted by remember(id) { mutableStateOf(false) }
    var terminalHeight by remember { mutableStateOf(Tokens.Size.TerminalDock) }

    fun toggleTerminal() {
        terminalShown = !terminalShown
        terminalMounted = terminalMounted || terminalShown
    }

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
    val sidePanels = !compact && panels
    val narrow = sidePanels && areaWidth != Dp.Unspecified && panelTakesOver(areaWidth)
    var environmentExpanded by remember(id) { mutableStateOf(false) }
    val environmentRunning = state.environments[id]?.state == EnvironmentState.Running
    val environmentFillsArea = sidePanels && panel == SidePanel.Environment && environmentExpanded && environmentRunning
    val takeover = (narrow && panel != null && !chatForced) || environmentFillsArea
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

    LaunchedEffect(panel, environmentRunning) {
        if (panel != SidePanel.Environment || !environmentRunning) {
            environmentExpanded = false
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
    var citations by remember(id) { mutableStateOf(emptyList<Annotation>()) }

    val attached = rememberAttachmentDrafts(id, draft.attachments)

    LaunchedEffect(id) {
        snapshotFlow { attached.drafts }.collect { files ->
            store.composerDrafts.update(id) { copy(attachments = files) }
        }
    }

    fun send(text: String) {
        store.send(id, store.withWalkthroughNotes(id, text), attached.drafts.map { it.file })
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

    LaunchedEffect(id, conversation.waitsToBeOpened) {
        if (conversation.waitsToBeOpened) {
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
    var outlinePhoneStage by remember(id) { mutableStateOf<OutlinePhoneStage?>(null) }
    val embedsOutline = platform?.embedsOtherSites != false
    val revealOutline: (OutlinePage) -> Boolean = { page ->
        outlinePage = page

        when {
            chat || !panels -> false
            compact -> {
                outlinePhoneStage = if (embedsOutline) OutlinePhoneStage.Document else OutlinePhoneStage.Tree

                true
            }
            else -> {
                openPanel(SidePanel.Outline)

                true
            }
        }
    }
    val showOutline: (OutlinePage) -> Unit = { page ->
        if (!revealOutline(page) || !embedsOutline) {
            platform?.openUrl(page.url)
        }
    }
    val selectOutline: (OutlinePage) -> Unit = { page ->
        outlinePage = page

        if (!embedsOutline) {
            platform?.openUrl(page.url)
        }
    }
    val outlineOnEntry = remember(id) { state.outlinePages[id] }
    val liveOutline = state.outlinePages[id]

    LaunchedEffect(liveOutline) {
        if (liveOutline != null && liveOutline != outlineOnEntry && !revealOutline(liveOutline)) {
            platform?.openUrl(liveOutline.url)
        }
    }

    val outlineAccounts = outlineIntegrations(state.chatToolServers, groupId)
    val shownOutlinePage = outlinePage ?: liveOutline
    val outlineBrowser = rememberOutlineBrowser(id, store.api, outlineAccounts, shownOutlinePage, panel == SidePanel.Outline || outlinePhoneStage != null)

    var providers by remember { mutableStateOf(emptyList<ModelProvider>()) }

    LaunchedEffect(Unit) {
        store.act { providers = api.modelProviders() }
    }

    val scope = rememberCoroutineScope()
    val voice by store.voice.state.collectAsState()
    val speechProvided = voice.privacy.hasSpeechToText()

    DisposableEffect(id) {
        onDispose {
            if (store.voice.state.value.voiceModeConversationId == id) {
                store.voice.close()
            }
        }
    }
    val dictation = remember(platform, state.experiments.voice, speechProvided) {
        when {
            !state.experiments.voice -> null
            else -> platform?.dictation() ?: platform?.audioRecorder()?.takeIf { speechProvided }?.let { ProviderDictation(it, api::transcribe, scope) }
        }
    }

    val node = if (chat) null else state.nodes[conversation.nodeId]
    val accountPicker = run {
        val favourites = rememberFavouriteModels(store)
        val login = node?.logins?.firstOrNull { it.agent == conversation.agent && it.account == conversation.account }
        val accounts = remember(conversation, state.nodes) {
            pickerAccounts(state.nodes.values, conversation.nodeId) { pickedNode, pickedLogin -> continueAccountId(conversation, pickedNode, pickedLogin) } +
                node?.let { signInAccounts(state.nodes.values, it, conversation.agent) }.orEmpty()
        }
        val options = remember(state.nodes, providers, node) { modelOptions(state.nodes.values, node, providers) }
        val sessionModel = remember(conversation, state.events[id], state.sessionUpdates[id], node) {
            sessionOptions(conversation, state.events[id].orEmpty(), state.sessionUpdates[id].orEmpty(), reportedModelChoices(node, conversation.agent, conversation.account)).currentModel
        }
        val anyModel = conversation.routing == Routing.AnyModel
        val current = if (chat) ModelChoice(providerId = conversation.providerId, model = conversation.model) else ModelChoice(agent = conversation.agent, model = sessionModel?.ifEmpty { null })

        ModelPickerControls(
            label = conversationModelLabel(conversation, sessionModel, options),
            tooltip = routedStatus(conversation, login),
            options = options,
            favourites = favourites.list,
            providers = providers,
            chosen = current.takeIf { !anyModel },
            anyChosen = anyModel,
            onFavourite = favourites.toggle,
            onPickAny = { store.act { store.continueOn(id, "$RoutingPrefix${Routing.AnyModel}") } },
            onPickModel = { option ->
                store.act {
                    when {
                        option.providerId != null && chat -> api.patchConversation(id, PatchConversationRequest(providerId = option.providerId, model = option.model))
                        option.providerId != null -> store.continueOn(id, "$ContinueChatPrefix${option.providerId}:${option.model}")
                        option.agent == conversation.agent && !chat -> api.setConfig(id, ConfigRequest(model = option.model))
                        else -> store.continueOn(id, "$ContinueModelPrefix${option.agent}:${option.model}")
                    }
                }
            },
            accounts = accounts,
            account = accounts.firstOrNull { it.id == currentContinueId(conversation) }?.id,
            onMove = { moveTo = it },
            onPickAccount = { picked, done ->
                store.act {
                    val chosen = picked ?: anyAccountId(conversation.agent)
                    val row = accounts.firstOrNull { it.id == chosen }

                    try {
                        when {
                            chosen.startsWith(SignInPrefix) && node != null -> {
                                val started = api.startLogin(node.id, conversation.agent, loginMethodFor(conversation.agent), chosen.removePrefix(SignInPrefix))
                                signIn = LoginSession(started.loginId, conversation.agent)
                            }
                            conversation.acpSessionId == null && chosen == "$ContinueAccountPrefix$DefaultAccountId" -> api.patchConversation(id, PatchConversationRequest(useDefaultAccount = true))
                            conversation.acpSessionId == null && chosen.startsWith(ContinueAccountPrefix) -> api.patchConversation(id, PatchConversationRequest(account = chosen.removePrefix(ContinueAccountPrefix)))
                            else -> store.continueOn(id, chosen)
                        }

                        done(null)
                    } catch (refused: ApiException) {
                        val target = row?.node

                        if (refused.code != "folder_missing" || target == null) {
                            throw refused
                        }

                        done(PickerRefusal(refused.message, chosen, target.id, target.name))
                    }
                }
            },
        )
    }
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
    val agentWorking = turnIsOpen(state.events[id].orEmpty()) || (conversation.agentActive && conversation.status == ConversationStatus.Working)
    var stopping by remember(id) { mutableStateOf(false) }

    LaunchedEffect(agentWorking) {
        if (!agentWorking) {
            stopping = false
        }
    }

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
    val unexplainedFailure = ThreadItem.Failure("failed", "The agent stopped with an error.").takeIf { conversation.status == ConversationStatus.Failed && thread.none { it is ThreadItem.Failure } }
    val initialMessage = initialQueued?.let { ThreadItem.UserMessage("queued:${it.id}", it.text, files = it.attachments) }
    val shownQueue = queue.filter { it.id != initialQueued?.id }
    var queueEditing by remember(id) { mutableStateOf<QueueEditing?>(null) }
    val loadAttachmentBytes = LocalAttachmentBytes.current

    fun leaveQueueEdit(editing: QueueEditing) {
        if (queueEditing != editing) {
            return
        }

        attached.replace(editing.stash.attachments)
        store.composerDrafts.update(id) { editing.stash.copy(attachments = attached.drafts) }
        queueEditing = null
    }

    val queueEdit = queueEditing?.let { editing ->
        QueueEdit(
            number = shownQueue.indexOfFirst { it.id == editing.queueId } + 1,
            onSave = { text ->
                store.act {
                    val uploaded = attached.drafts.map { api.uploadAttachment(id, it.file.name, it.file.mediaType, it.file.bytes) }

                    api.editQueued(id, editing.queueId, PatchQueueRequest(text = text, attachments = uploaded, editing = false))
                    leaveQueueEdit(editing)
                }
            },
            onCancel = {
                store.act {
                    api.editQueued(id, editing.queueId, PatchQueueRequest(editing = false))
                    leaveQueueEdit(editing)
                }
            },
        )
    }

    LaunchedEffect(queueEditing, shownQueue.map { it.id }) {
        val editing = queueEditing

        if (editing != null && shownQueue.none { it.id == editing.queueId }) {
            leaveQueueEdit(editing)
        }
    }

    val annotate = remember(id) {
        AnnotateActions(
            onAttach = { added -> citations = citations + added },
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

    val outerAppViewer = LocalAppViewer.current
    val appViewer = outerAppViewer?.let { outer ->
        { uuid: String ->
            if (!compact && !chat && panels) {
                viewedApp = uuid
                openPanel(SidePanel.Apps)
            } else {
                outer(uuid)
            }
        }
    }

    LaunchedEffect(id, state.chatToolServers) {
        if (appsLinkVisible(state.chatToolServers)) {
            runCatching { store.refreshApps() }
        }
    }

    var openedWorkflowId by remember(id) { mutableStateOf<String?>(null) }
    val openedWorkflow = openedWorkflowId?.let { state.workflows[it] }
    val workflowControl: (String, WorkflowControlRequest) -> Unit = { workflowId, request -> store.act { store.controlWorkflow(workflowId, request) } }
    val conversationWorkflows = ConversationWorkflows(
        workflows = state.workflows.values.filter { it.conversationId == id }.sortedBy { it.createdAt },
        onOpen = if (chat || !panels) null else { workflowId -> openedWorkflowId = workflowId; openPanel(SidePanel.Subagents) },
        onControl = workflowControl,
    )

    LaunchedEffect(id) {
        runCatching { store.refreshWorkflows() }
    }

    val agentOpener: (String) -> (() -> Unit)? = { agentId -> state.conversations.values.firstOrNull { it.parentId == id && it.agentId == agentId }?.let { agentConversation -> { onOpenConversation(agentConversation.id) } } }

    var browsingPath by remember(id) { mutableStateOf<String?>(null) }
    val pathOpener: ((String) -> Unit)? = if (chat || node == null) null else { path ->
        val rooted = path.startsWith("/") || path.startsWith("~")
        val full = if (rooted) path else conversation.cwd.trimEnd('/') + "/" + path.removePrefix("./")

        if (platform?.localNodeId() == node.id) {
            platform.revealPath(pathWithoutLine(full))
        } else {
            browsingPath = full
        }
    }

    browsingPath?.let { path ->
        node?.let { FolderBrowserDialog(store, it.id, it.name, parentFolder(path), onChoose = { browsingPath = null }, onDismiss = { browsingPath = null }) }
    }

    CompositionLocalProvider(LocalBroughtIn provides BroughtInActions(store, state, onOpenConversation), LocalThreadSubagents provides ThreadSubagents(store, state, id, onOpenConversation), LocalPathOpener provides pathOpener, LocalAnnotate provides annotate, LocalGeneratedUiActions provides generatedActions, LocalAttachmentBytes provides store::attachmentBytes, LocalArtifactOpener provides artefactOpener, LocalAppViewer provides appViewer, LocalAgentOpener provides agentOpener, LocalConversationWorkflows provides conversationWorkflows, LocalOutlineOpener provides showOutline, LocalOutlinePage provides shownOutlinePage, LocalIdeTarget provides ideTarget) {
        val walkthrough = state.walkthroughs[id]?.takeIf { !it.dismissed && !chat }

        val density = LocalDensity.current

        val panelContent: @Composable () -> Unit = {
            when (panel) {
                SidePanel.Subagents -> SubagentsPanel(subagentSections(state.conversations.values, id, state.workflows.values), id, subagentParent, openedWorkflow, { openedWorkflowId = it }, workflowControl, onOpenConversation, store::closeSession, state.conversations.values)
                SidePanel.Git -> key(conversation.id) { GitPanel(store, conversation, Modifier.fillMaxSize(), onOpenDiff = { store.openDiffTab(conversation.id, it) }) }
                SidePanel.Artefacts -> platform?.let { ArtefactsPanel(store, state, it, id, viewedArtefact, { viewedArtefact = it }, onOpenArtefact, Modifier.fillMaxSize()) }
                SidePanel.Apps -> platform?.let { AppsPanel(store, state, it, id, viewedApp, { viewedApp = it }, Modifier.fillMaxSize()) }
                SidePanel.Outline -> platform?.let { OutlinePanel(outlineBrowser, it, outlineAccounts, shownOutlinePage, selectOutline, onClose = { openPanel(null) }) }
                SidePanel.PullRequest -> key(conversation.id) { ConversationPullRequestPanel(store, state, conversation, Modifier.fillMaxSize()) }
                SidePanel.Environment -> key(conversation.id) { EnvironmentPanel(store, state, conversation, environmentFillsArea, { environmentExpanded = it }, { openPanel(null) }, onOpenNode, onOpenConversation, onOpenNodes) }
                null -> Unit
            }
        }

        val terminalAvailable = !chat && !conversation.scratch

        LaunchedEffect(id, terminalAvailable) {
            if (terminalAvailable) {
                TerminalShortcut.presses.collect { toggleTerminal() }
            }
        }

        Row(
            Modifier
                .fillMaxSize()
                .onPreviewKeyEvent { event -> terminalAvailable && TerminalShortcut.handle(event) }
                .onSizeChanged { areaWidth = with(density) { it.width.toDp() } },
        ) {
            Column(
                Modifier
                    .then(if (takeover) Modifier.width(0.dp).clearAndSetSemantics {} else Modifier.weight(1f))
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
                Column(if (compact && terminalShown) Modifier.height(0.dp).clipToBounds().clearAndSetSemantics {} else Modifier.weight(1f).fillMaxWidth()) {
                ConversationHeader(
                    slots = header,
                    conversation = conversation,
                    node = node,
                    chat = chat,
                    compact = compact,
                    onNode = { nodeSheetOpen = true },
                    onOpenNodes = onOpenNodes,
                    moveSheet = { dismiss -> MoveSheet(store, state, conversation, dismiss, onOpenConversation) },
                    ide = ideControls,
                    terminalShown = terminalShown,
                    onTerminal = ::toggleTerminal,
                    onGit = { openPanel(SidePanel.Git) },
                    onEnvironment = { phoneEnvironment = true },
                    project = state.projects[conversation.projectId],
                    projects = state.projects.values.filter { it.nodeId == conversation.nodeId }.sortedBy { it.name },
                    nodes = state.nodes,
                    onProject = { projectId -> store.setConversationProject(conversation, projectId) },
                    onSetUp = { store.act { store.createProject(CreateProjectRequest(conversation.nodeId, path = conversation.cwd)) } },
                    onRename = { title -> store.rename(conversation, title) },
                    onPin = { store.pin(conversation, conversation.pinnedAt == null) },
                    onSnooze = { until -> store.snooze(conversation, until) },
                    manager = conversation.managedBy?.let { state.conversations[it] }?.takeIf { it.archivedAt == null },
                    onTakeOver = { store.takeOver(conversation) },
                    onCloseSession = { store.closeSession(conversation) },
                    onArchive = {
                        val archiving = conversation.archivedAt == null

                        store.act {
                            api.patchConversation(id, PatchConversationRequest(archived = archiving))

                            if (archiving) {
                                onArchived()
                            }
                        }
                    },
                    onDelete = {
                        store.act {
                            api.deleteConversation(id)
                            onArchived()
                        }
                    },
                    watches = watches,
                    watchStatus = { watched -> state.conversations[watched]?.status },
                    onOpenWatch = onOpenConversation,
                    onStopWatch = { watch -> store.act { api.deleteWatch(watch.id) } },
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
                    continueOn = accountPicker,
                )

                conversation.snoozedUntil?.let { until -> SnoozedLine(until, onWake = { store.snooze(conversation, null) }) }

                CompositionLocalProvider(LocalResponseActions provides responses, LocalPausedWorkflow provides PausedWorkflow(conversation.agent, limitActions)) {
                    ThreadView(
                        items = thread + listOfNotNull(unexplainedFailure) + listOfNotNull(initialMessage) + delivering.map { ThreadItem.UserMessage("sending", it) },
                        hasOlder = state.hasOlderEvents(id),
                        loadingOlder = id in state.loadingOlderEvents,
                        onLoadOlder = { store.act { store.loadOlder(id) } },
                        onAnswer = { requestId, optionId -> store.act { api.answerPermission(id, requestId, optionId) } },
                        onStopWorkflow = { store.act { api.cancel(id) } },
                        modifier = Modifier.weight(1f),
                        onSuggest = ::send,
                        onRetry = if (conversation.status == ConversationStatus.Failed) ({ store.act { api.retryConversation(id) } }) else null,
                        onDismissFailure = if (conversation.status == ConversationStatus.Failed) ({ store.dismissFailure(conversation) }) else null,
                        limitActions = if (conversation.status == ConversationStatus.Failed) limitActions else null,
                        agent = conversation.agent,
                        dismissedPins = dismissedPins,
                        onUnpin = { key ->
                            store.pinDismissals.dismiss(id, key)
                            dismissedPins = store.pinDismissals[id]
                        },
                        waitingForAnswer = conversation.status == ConversationStatus.Input,
                    )
                }

                state.schedules.values.filter { it.proposed && it.conversationId == id }.forEach { ProposedScheduleCard(store, it, state) }
                state.actionProposals.values.filter { it.conversationId == id && it.state == "proposed" }.sortedBy { it.createdAt }.forEach { ProposedActionCard(store, state, it) }
                state.assistantChanges.values.filter { it.conversationId == id && it.state == "proposed" }.sortedBy { it.createdAt }.forEach { AssistantChangeCard(store, state, it, onOpenSettings = onManageActions) }

                val parentId = conversation.parentId

                if (conversation.isSubagent && parentId != null) {
                    Row(Modifier.fillMaxWidth().padding(Tokens.Space.S3), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3), verticalAlignment = Alignment.CenterVertically) {
                        Label("A subagent's conversation is read-only.", Modifier.weight(1f), color = MyTheme.colors.textMuted)
                        ConversationUsageBar(api, id, state.events[id].orEmpty().count { it.kind == mycelium.api.EventKinds.TurnEnd })
                        MyButton({ onOpenConversation(parentId) }, text = "Open the conversation it belongs to", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
                    }
                } else {
                    key(id, queueEditing?.queueId) {
                        Composer(
                            conversation = conversation,
                            usage = { ConversationUsageBar(api, id, state.events[id].orEmpty().count { it.kind == mycelium.api.EventKinds.TurnEnd }) },
                            working = agentWorking,
                            stopping = stopping,
                            queue = shownQueue.map { if (it.id == queueEditing?.queueId) it.copy(editing = true) else it },
                            sending = sending.drop(delivering.size),
                            onSend = ::send,
                            pendingCitations = citations,
                            onCitationPlaced = { placed -> citations = citations - placed },
                            attachments = attached.controls(),
                            assistantActions = assistantActions,
                            dictation = dictation,
                            initialText = TextFieldValue(draft.text, TextRange(draft.cursor)),
                            onTextChange = { typed -> store.composerDrafts.update(id) { copy(text = typed.text, cursor = typed.selection.end) } },
                            initialImages = draft.images,
                            onImagesChange = { held -> store.composerDrafts.update(id) { copy(images = held) } },
                            accountPicker = accountPicker,
                        onStop = {
                            stopping = true
                            store.act {
                                try {
                                    api.cancel(id)
                                } catch (failure: Exception) {
                                    stopping = false
                                    throw failure
                                }
                            }
                        },
                            onSendQueuedNow = { queueId -> store.act { api.sendQueuedNow(id, queueId) } },
                            onSendAllQueuedNow = { store.act { api.sendAllQueuedNow(id) } },
                            onEditQueued = { item ->
                                store.act {
                                    val files = item.attachments.map { held -> PickedFile(held.name, held.mediaType, loadAttachmentBytes(held.sha256) ?: error("${held.name} could not be loaded. Try again.")) }
                                    api.editQueued(id, item.id, PatchQueueRequest(editing = true))

                                    val stash = store.composerDrafts[id].copy(attachments = attached.drafts)

                                    attached.replace(emptyList())
                                    attached.add(files)
                                    store.composerDrafts.update(id) { ComposerDraft(text = item.text, cursor = item.text.length, attachments = attached.drafts) }
                                    queueEditing = QueueEditing(item.id, stash)
                                }
                            },
                            onMoveQueued = { queueId, index ->
                                store.act {
                                    val neighbour = shownQueue[index]

                                    api.moveQueued(id, queueId, queue.indexOfFirst { it.id == neighbour.id })
                                }
                            },
                            queueEdit = queueEdit,
                            onRemoveQueued = { queueId -> store.act { api.removeQueued(id, queueId) } },
                            options = remember(conversation, state.events[id], state.sessionUpdates[id], state.nodes[conversation.nodeId]) { sessionOptions(conversation, state.events[id].orEmpty(), state.sessionUpdates[id].orEmpty(), reportedModelChoices(state.nodes[conversation.nodeId], conversation.agent, conversation.account)) },
                            commands = remember(state.events[id], state.sessionUpdates[id]) { slashCommands(state.events[id].orEmpty(), state.sessionUpdates[id].orEmpty()) },
                            onSetMode = { mode -> store.act { api.setConfig(id, ConfigRequest(mode = mode)) } },
                            permissions = SessionPermissions(sessionGrants(state.events[id].orEmpty()), if (chat) ({ tool -> store.act { api.revokePermission(id, tool) } }) else null),
                            onSetModel = { model -> store.act { api.setConfig(id, ConfigRequest(model = model)) } },
                            onSearchFiles = { query -> api.files(id, query) },
                            onSearchReferences = { query -> forgeReferences(api.issues().issues, api.pullRequests().pullRequests, query) },
                            onSetSetting = { setting, value -> store.act { api.setConfig(id, ConfigRequest(config = mapOf(setting to JsonPrimitive(value)))) } },
                            onSideChat = { kind, text -> store.act { onOpenConversation(api.startSideChat(id, sideKind(kind), text.ifEmpty { null }).id) } },
                            onTeachMe = { on -> store.act { api.patchConversation(id, PatchConversationRequest(teachMe = on)) } },
                            voiceMode = if (!voice.privacy.voiceEnabled) null else {
                                val here = voice.voiceModeConversationId == id

                                VoiceModeControls(
                                    phase = voice.phase.takeIf { here },
                                    heard = if (here) voice.heard else "",
                                    problem = (voice.phase as? VoicePhase.Problem)?.takeIf { here }?.let { voiceProblemMessage(it.problem, platform?.opensMicrophoneSettings == true) },
                                    onToggle = { if (here) store.voice.close() else store.voice.enterVoiceMode(id) },
                                    onSendHeard = store.voice::stopListening,
                                )
                            },
                            todos = remember(thread, state.events[id]) { currentTodos(thread).takeIf { todosAreShown(it, turnIsOpen(state.events[id].orEmpty())) }.orEmpty() },
                            onSaveTodos = { todos, tellAgent -> store.act { store.saveTodos(id, todos, tellAgent) } },
                            goal = GoalControls(
                                goal = state.goals[id],
                                onSet = { text, limits -> store.act { store.setGoal(id, text, limits) } },
                                onPause = { store.act { store.pauseGoal(id, true) } },
                                onResume = { store.act { store.pauseGoal(id, false) } },
                                onClear = { store.act { store.clearGoal(id) } },
                                onSendAgain = { store.act { store.sendGoalAgain(id) } },
                                onNudge = { store.act { store.nudgeGoal(id) } },
                                onContinue = { note -> store.act { store.continueGoal(id, note) } },
                                agent = conversation.agent,
                            ),
                        )
                    }
                }
                }

                if (terminalAvailable && terminalMounted) {
                    Column(if (terminalShown) (if (compact) Modifier.weight(1f) else Modifier.height(terminalHeight)) else Modifier.height(0.dp).clearAndSetSemantics {}) {
                        if (!compact) {
                            ResizeHandle("Resize the terminal", Orientation.Vertical) { moved -> terminalHeight = (terminalHeight - moved).coerceIn(Tokens.Size.TerminalDockMin, Tokens.Size.TerminalDockMax) }
                        }

                        key(id) {
                            TerminalPanel(store, state, conversation, Modifier.weight(1f).fillMaxWidth(), onExited = { terminalShown = false; terminalMounted = false }, onClose = { terminalShown = false })
                        }
                    }
                }
            }

            if (!compact && panels) {
                if (environmentFillsArea) {
                    Box(Modifier.weight(1f).fillMaxHeight()) {
                        panelContent()
                    }
                } else if (takeover) {
                    Column(Modifier.weight(1f).fillMaxHeight().background(MyTheme.colors.bgPanel)) {
                        Box(Modifier.weight(1f).fillMaxWidth()) {
                            CompositionLocalProvider(LocalPanelBack provides { chatForced = true }) {
                                panelContent()
                            }
                        }
                    }
                } else if (panel != null && !narrow) {
                    ResizeHandle("Resize the side panel") { moved -> resizePanel(panelWidth - moved) }

                    Box(Modifier.width(shownPanelWidth(panelWidth, areaWidth, browserPanel)).fillMaxHeight()) {
                        panelContent()
                    }
                }

                Rail(if (narrow && chatForced) null else panel, !chat && outlineAccounts.isNotEmpty(), conversationApps(state, id).isNotEmpty(), conversation.pullRequestNumber != null, conversation.scratch || chat) { chosen ->
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
                SidePanel.Subagents -> if (openedWorkflow != null) {
                    WorkflowPanelDetail(openedWorkflow, state.conversations.values, { openedWorkflowId = null }, onOpenConversation) { workflowControl(openedWorkflow.id, it) }
                } else {
                    SubagentsList(subagentSections(state.conversations.values, id, state.workflows.values), id, subagentParent, { openedWorkflowId = it }, onOpenConversation, store::closeSession)
                }
                SidePanel.Git -> key(conversation.id) { GitPanel(store, conversation, Modifier.fillMaxWidth().height(Tokens.Size.GitSheet)) }
                SidePanel.Artefacts -> platform?.let { ArtefactsPanel(store, state, it, id, viewedArtefact, { viewedArtefact = it }, { openPanel(null); onOpenArtefact(it) }, Modifier.fillMaxWidth().height(Tokens.Size.GitSheet)) }
                SidePanel.PullRequest -> key(conversation.id) { ConversationPullRequestPanel(store, state, conversation, Modifier.fillMaxWidth().height(Tokens.Size.GitSheet)) }
                SidePanel.Apps, SidePanel.Outline, SidePanel.Environment, null -> Unit
            }
        }
    }

    outlinePhoneStage?.takeIf { compact && !chat && panels }?.let { stage ->
        FullScreenSheet({ outlinePhoneStage = if (stage == OutlinePhoneStage.Document) OutlinePhoneStage.Tree else null }) {
            platform?.let { OutlinePhone(outlineBrowser, it, outlineAccounts, shownOutlinePage, stage, { next -> outlinePhoneStage = next }, selectOutline, onClose = { outlinePhoneStage = null }) }
        }
    }

    if (compact && phoneEnvironment) {
        FullScreenSheet({ phoneEnvironment = false }) {
            EnvironmentPanel(store, state, conversation, true, { phoneEnvironment = it }, { phoneEnvironment = false }, onOpenNode, onOpenConversation, onOpenNodes)
        }
    }

    if (compact && nodeSheetOpen) {
        NodeSheet(node, onDismiss = { nodeSheetOpen = false }, onOpenNodes = { nodeSheetOpen = false; onOpenNodes() }, onMove = { nodeSheetOpen = false; moveOpen = true })
    }

    if (compact && moveOpen) {
        MoveSheet(store, state, conversation, { moveOpen = false }, onOpenConversation)
    }

    moveTo?.let { target -> MoveSheet(store, state, conversation, { moveTo = null }, onOpenConversation, target) }

    signIn?.let { session -> AccountSignInDialog(store, session, onClose = { signIn = null }) }
}

@Composable
private fun ConversationHeader(
    slots: HeaderSlots?,
    conversation: Conversation,
    node: NodeSummary?,
    chat: Boolean,
    compact: Boolean,
    onNode: () -> Unit,
    onOpenNodes: () -> Unit,
    moveSheet: @Composable (onDismiss: () -> Unit) -> Unit,
    ide: IdeControls?,
    terminalShown: Boolean,
    onTerminal: () -> Unit,
    onGit: () -> Unit,
    onEnvironment: () -> Unit,
    project: Project?,
    projects: List<Project>,
    nodes: Map<String, NodeSummary>,
    onProject: (String?) -> Unit,
    onSetUp: () -> Unit,
    onRename: (String) -> Unit,
    onPin: () -> Unit,
    onSnooze: (String?) -> Unit,
    manager: Conversation?,
    onTakeOver: () -> Unit,
    onCloseSession: () -> Unit,
    onArchive: () -> Unit,
    onDelete: () -> Unit,
    watches: List<Watch>,
    watchStatus: (String) -> ConversationStatus?,
    onOpenWatch: (String) -> Unit,
    onStopWatch: (Watch) -> Unit,
) {
    val colors = MyTheme.colors
    var confirmingDelete by remember(conversation.id) { mutableStateOf(false) }
    var renaming by remember(conversation.id) { mutableStateOf(false) }
    var draft by remember(conversation.title) { mutableStateOf(conversation.title) }
    var menuOpen by remember { mutableStateOf(false) }
    var nodeMenuOpen by remember { mutableStateOf(false) }
    var moveOpen by remember { mutableStateOf(false) }
    var projectMenuOpen by remember { mutableStateOf(false) }
    var snoozeMenuOpen by remember { mutableStateOf(false) }
    var ideMenuOpen by remember { mutableStateOf(false) }
    var watchingMenuOpen by remember { mutableStateOf(false) }
    val archived = conversation.archivedAt != null
    val density = LocalDensity.current
    val titleSource = remember { MutableInteractionSource() }
    var barWidth by remember { mutableStateOf(0.dp) }
    var placeWidth by remember { mutableStateOf(0.dp) }
    var ideWidth by remember { mutableStateOf(0.dp) }
    val fixedWidth = Tokens.Size.ControlMd * 2 + Tokens.Space.S3 + Tokens.Space.S2 * 7
    val reserved = fixedWidth + Tokens.Size.HeaderTitleMin
    val measured = barWidth > 0.dp
    val ideInMore = !compact && ide != null && measured && barWidth < reserved + placeWidth + ideWidth
    val placeInMore = !compact && measured && barWidth < reserved + placeWidth
    val titleMax = (barWidth - fixedWidth - (if (placeInMore) 0.dp else placeWidth) - (if (ideInMore || ide == null) 0.dp else ideWidth)).coerceIn(Tokens.Size.HeaderTitleMin, Tokens.Size.Measure / 2)

    val nodeMenu: @Composable () -> Unit = {
        if (nodeMenuOpen) {
            MenuPopup(onDismiss = { nodeMenuOpen = false }) {
                if (!conversation.scratch) {
                    MenuItem("Move", onClick = { nodeMenuOpen = false; moveOpen = true }, leading = MyIcon.Move)
                }

                MenuItem("All nodes", onClick = { nodeMenuOpen = false; onOpenNodes() }, leading = MyIcon.Server)
            }
        }

        if (moveOpen) {
            moveSheet { moveOpen = false }
        }
    }

    Bar(Modifier.onSizeChanged { barWidth = with(density) { it.width.toDp() } }, startPadding = if (compact) Tokens.Space.S1 else Tokens.Space.S3) {
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
                (if (compact) Modifier.weight(1f) else Modifier.widthIn(max = titleMax))
                    .heightIn(min = touchMin(0.dp))
                    .pressable(
                        { if (compact) menuOpen = true else renaming = true },
                        description = if (compact) "${conversation.title}, rename or archive" else "Rename conversation ${conversation.title}",
                        shape = ControlShape,
                        source = titleSource,
                    )
                    .padding(horizontal = Tokens.Space.S1),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Label(conversation.title, style = Tokens.Type.Title, maxLines = 1)
                Tooltip(conversation.title, titleSource)
            }
        }

        if (compact && chat) {
            Box(Modifier.weight(0.001f))
            WatchingButton(watches, watchStatus, onOpenWatch, onStopWatch, compact = true)
            MyButton(onEnvironment, icon = MyIcon.Desktop, description = "Environment", muted = true)
        } else if (compact) {
            MyButton(onNode, icon = node?.let(::nodeIcon) ?: MyIcon.Server, description = "Running on ${node?.name ?: "an unknown node"}", muted = true)
            WatchingButton(watches, watchStatus, onOpenWatch, onStopWatch, compact = true)

            if (!conversation.scratch) {
                MyButton(onTerminal, icon = MyIcon.Terminal, description = if (terminalShown) "Hide terminal" else "Show terminal", muted = true, selected = terminalShown)
                MyButton(onGit, icon = MyIcon.Branch, description = "Git", muted = true)
            }

            MyButton(onEnvironment, icon = MyIcon.Desktop, description = "Environment", muted = true)
        } else {
            if (!placeInMore) {
                Row(Modifier.onSizeChanged { placeWidth = with(density) { it.width.toDp() } }, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                    if (!chat && !conversation.scratch) {
                        ProjectBreadcrumb(project, projects, nodes, onProject, setUpFolder = conversation.cwd.takeIf { project == null }, onSetUp = onSetUp)
                    }

                    if (node != null) {
                        Box {
                            MyButton({ nodeMenuOpen = true }, text = node.name, icon = nodeIcon(node), trailingIcon = MyIcon.ChevronDown, description = "Running on ${node.name}, open the node menu", size = ButtonSize.Sm, muted = true)
                            nodeMenu()
                        }
                    }

                    WatchingButton(watches, watchStatus, onOpenWatch, onStopWatch)
                }
            }

            if (archived) {
                Tag("Archived")
            }

            Box(Modifier.weight(1f))
        }

        if (ide != null && !compact && !ideInMore) {
            Box(Modifier.onSizeChanged { ideWidth = with(density) { it.width.toDp() } }) {
                SplitButton(
                    "Open in ${ide.main.label}",
                    { ide.open(ide.main) },
                    ide.others.map { SplitAlternative("Open in ${it.label}", { ide.open(it) }) } + SplitAlternative("IDE settings", ide.onSettings, separated = true),
                    variant = ButtonVariant.Ghost,
                    size = ButtonSize.Sm,
                    menuLabel = ide.where,
                    icon = MyIcon.Code,
                    muted = true,
                )
            }
        }

        Box {
            if (!compact || slots != null) {
                MyButton({ menuOpen = true }, icon = MyIcon.MoreVertical, description = if (slots != null) "More for ${conversation.title}" else "More", muted = true)
            }

            if (placeInMore) {
                nodeMenu()
            }

            if (watchingMenuOpen && watches.isNotEmpty()) {
                WatchingMenu(watches, watchStatus, onOpenWatch, onStopWatch, onDismiss = { watchingMenuOpen = false })
            }

            if (ideMenuOpen && ide != null) {
                MenuPopup(onDismiss = { ideMenuOpen = false }) {
                    MenuLabel(ide.where)
                    MenuItem("Open in ${ide.main.label}", onClick = { ideMenuOpen = false; ide.open(ide.main) }, leading = MyIcon.Code)
                    ide.others.forEach { other -> MenuItem("Open in ${other.label}", onClick = { ideMenuOpen = false; ide.open(other) }, leading = MyIcon.Code) }
                    Hairline(Modifier.padding(vertical = Tokens.Space.S1))
                    MenuItem("IDE settings", onClick = { ideMenuOpen = false; ide.onSettings() }, leading = MyIcon.Settings)
                }
            }

            if (menuOpen) {
                MenuPopup(onDismiss = { menuOpen = false }) {
                    if (placeInMore && !chat && !conversation.scratch) {
                        MenuItem("Project", onClick = { menuOpen = false; projectMenuOpen = true }, hint = project?.name ?: "None", leading = MyIcon.Folder)
                    }

                    if (placeInMore && node != null) {
                        MenuItem("Running on", onClick = { menuOpen = false; nodeMenuOpen = true }, hint = node.name, leading = nodeIcon(node))
                    }

                    if (placeInMore && watches.isNotEmpty()) {
                        MenuItem("Watching", onClick = { menuOpen = false; watchingMenuOpen = true }, hint = watches.size.toString(), leading = MyIcon.Eye)
                    }

                    if (ideInMore && ide != null) {
                        MenuItem("Open in ${ide.main.label}", onClick = { menuOpen = false; ideMenuOpen = true }, hint = ide.where, leading = MyIcon.Code)
                    }

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
                    MenuItem(if (conversation.snoozedUntil == null) "Snooze" else "Wake now", onClick = { menuOpen = false; if (conversation.snoozedUntil == null) snoozeMenuOpen = true else onSnooze(null) }, leading = MyIcon.Clock)

                    if (manager != null) {
                        MenuItem("Take over", onClick = { menuOpen = false; onTakeOver() }, hint = "from ${manager.title.ifBlank { manager.id }}", leading = MyIcon.Hand)
                    }

                    if (!chat && !conversation.scratch) {
                        MenuItem(if (terminalShown) "Hide terminal" else "Show terminal", onClick = { menuOpen = false; onTerminal() }, hint = "Ctrl+`", leading = MyIcon.Terminal)
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

    if (snoozeMenuOpen) {
        SnoozeMenu(onSnooze = onSnooze, onDismiss = { snoozeMenuOpen = false })
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

private enum class SidePanel { Subagents, Git, PullRequest, Artefacts, Apps, Outline, Environment }

@Composable
private fun Rail(open: SidePanel?, hasOutline: Boolean, hasApps: Boolean, hasPullRequest: Boolean, environmentOnly: Boolean, onToggle: (SidePanel) -> Unit) {
    val colors = MyTheme.colors

    Row(Modifier.fillMaxHeight()) {
        VerticalHairline()

        Column(
            Modifier.width(Tokens.Size.Rail).fillMaxHeight().background(colors.bgChrome).padding(top = Tokens.Space.S1),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            if (!environmentOnly) {
                MyButton({ onToggle(SidePanel.Subagents) }, icon = MyIcon.Agent, description = if (open == SidePanel.Subagents) "Close subagents and workflows" else "Open subagents and workflows", muted = true, selected = open == SidePanel.Subagents)

                MyButton({ onToggle(SidePanel.Git) }, icon = MyIcon.Branch, description = if (open == SidePanel.Git) "Close git" else "Open git", muted = true, selected = open == SidePanel.Git)
                if (hasPullRequest) {
                    MyButton({ onToggle(SidePanel.PullRequest) }, icon = MyIcon.PullRequest, description = if (open == SidePanel.PullRequest) "Close pull request" else "Open pull request", muted = true, selected = open == SidePanel.PullRequest)
                }

                MyButton({ onToggle(SidePanel.Artefacts) }, icon = MyIcon.Layers, description = if (open == SidePanel.Artefacts) "Close artefacts" else "Open artefacts", muted = true, selected = open == SidePanel.Artefacts)
                if (hasApps) {
                    MyButton({ onToggle(SidePanel.Apps) }, icon = MyIcon.AppWindow, description = if (open == SidePanel.Apps) "Close apps" else "Open apps", muted = true, selected = open == SidePanel.Apps)
                }
            }

            MyButton({ onToggle(SidePanel.Environment) }, icon = MyIcon.Desktop, description = if (open == SidePanel.Environment) "Close the environment panel" else "Open environment", muted = true, selected = open == SidePanel.Environment)

            if (hasOutline) {
                MyButton({ onToggle(SidePanel.Outline) }, icon = MyIcon.BookOpen, description = if (open == SidePanel.Outline) "Close Outline" else "Open Outline", muted = true, selected = open == SidePanel.Outline)
            }
        }
    }
}

@Composable
private fun SubagentsPanel(sections: List<SubagentSection>, currentId: String, parent: Conversation?, openedWorkflow: Workflow?, onOpenWorkflow: (String?) -> Unit, onControl: (String, WorkflowControlRequest) -> Unit, onOpen: (String) -> Unit, onStop: (Conversation) -> Unit, conversations: Collection<Conversation>) {
    if (openedWorkflow != null) {
        WorkflowPanelDetail(openedWorkflow, conversations, { onOpenWorkflow(null) }, onOpen) { onControl(openedWorkflow.id, it) }

        return
    }

    Column(Modifier.fillMaxSize().background(MyTheme.colors.bgPanel).semantics { contentDescription = "Subagents and workflows" }) {
        Bar {
            PanelTitle("Subagents and workflows")
        }

        Column(Modifier.weight(1f).verticalScrollWithBar().padding(vertical = Tokens.Space.S2)) {
            SubagentsList(sections, currentId, parent, onOpenWorkflow, onOpen, onStop)
        }
    }
}

@Composable
private fun SubagentsList(sections: List<SubagentSection>, currentId: String, parent: Conversation?, onOpenWorkflow: (String) -> Unit, onOpen: (String) -> Unit, onStop: (Conversation) -> Unit) {
    val colors = MyTheme.colors

    if (parent != null) {
        BackButton(parent.title, { onOpen(parent.id) }, Modifier.padding(horizontal = Tokens.Space.S2), showText = true)
    }

    if (sections.isEmpty()) {
        Label("This conversation has no subagents or workflows.", Modifier.padding(horizontal = Tokens.Space.S3), color = colors.textMuted)
    }

    sections.forEachIndexed { index, section ->
        if (index > 0) {
            Hairline(Modifier.padding(vertical = Tokens.Space.S1_5))
        }

        if (sections.size > 1) {
            Label(section.owner.title, Modifier.padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S1).semantics { heading() }, Tokens.Type.MetaStrong, colors.textMuted, maxLines = 1)
        }

        section.history.forEach { entry ->
            val menu = rememberRowMenuState()
            val workflow = (entry as? HistoryEntry.Run)?.workflow
            val subagent = (entry as? HistoryEntry.Subagent)?.conversation
            val look = workflow?.let { workflowStatusLook(it.status) }
            val progress = workflow?.takeIf { it.status == WorkflowStatuses.Done && it.plan.steps.isNotEmpty() }?.let { ", ${it.steps.count { step -> step.status == WorkflowStepStatuses.Done || step.status == WorkflowStepStatuses.Approved }} of ${it.plan.steps.size} steps" }.orEmpty()
            val state = if (workflow != null && look != null) look.label + progress else statusSectionName(subagent!!.status)
            val attention = look?.tone == Tone.Attention || subagent?.status == ConversationStatus.Input
            val title = workflow?.title ?: subagent!!.title
            val createdAt = entry.createdAt
            val running = subagent?.status == ConversationStatus.Working || subagent?.status == ConversationStatus.Input
            val open = { if (workflow != null) onOpenWorkflow(workflow.id) else onOpen(subagent!!.id) }

            Row(
                Modifier
                    .fillMaxWidth()
                    .rowContextMenu(menu)
                    .pressable(open, selected = subagent != null && subagent.id == currentId, shape = BlockShape, description = if (workflow != null) "Workflow $title, $state, started ${ago(createdAt)}" else "$title, $state, started ${ago(createdAt)}")
                    .padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S1_5),
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Icon(if (workflow != null) MyIcon.Workflow else MyIcon.Agent, colors.textMuted, Tokens.Size.IconSm)

                Column(Modifier.weight(1f)) {
                    Label(title, maxLines = 1)
                    val line = buildAnnotatedString {
                        append("${if (workflow != null) "Workflow" else "Subagent"} · ")

                        if (attention) {
                            withStyle(SpanStyle(color = MyTheme.colors.statusAttention)) { append(state) }
                        } else {
                            append(state)
                        }
                    }
                    BasicText(line, style = Tokens.Type.Meta.themed().copy(color = colors.textMuted), maxLines = 1, overflow = TextOverflow.Ellipsis)
                }

                Label(relativeTime(createdAt), style = Tokens.Type.Meta, color = colors.textFaint, maxLines = 1)

                if (subagent != null) {
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
