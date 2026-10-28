package mycelium.ui

import androidx.compose.animation.core.Animatable
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.focusable
import androidx.compose.foundation.gestures.Orientation
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.draggable
import androidx.compose.foundation.gestures.rememberDraggableState
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.requiredWidth
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.backhandler.BackHandler
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onKeyEvent
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.input.pointer.PointerEventType
import androidx.compose.ui.input.pointer.isSecondaryPressed
import androidx.compose.ui.input.pointer.pointerHoverIcon
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.pointer.positionChange
import androidx.compose.ui.input.pointer.util.VelocityTracker
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import kotlin.math.absoluteValue
import kotlin.math.roundToInt
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import mycelium.Platform
import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.api.Me
import mycelium.api.ModelProviderUsage
import mycelium.state.VoiceAssistantKey
import mycelium.state.VoicePhase
import mycelium.state.wakePhraseName
import mycelium.api.NodeSummary
import mycelium.api.PatchConversationRequest
import mycelium.api.Project
import mycelium.api.UseResetRequest
import mycelium.state.AllGroups
import mycelium.state.AppState
import mycelium.state.BreakPhase
import mycelium.state.Choice
import mycelium.state.HoursAction
import mycelium.state.HoursView
import mycelium.state.IdeLauncher
import mycelium.state.NoGroup
import mycelium.state.PinnedName
import mycelium.state.SidebarSection
import mycelium.state.Store
import mycelium.state.UnnamedAccount
import mycelium.state.agentAccounts
import mycelium.state.agentName
import mycelium.state.appsLinkVisible
import mycelium.state.chatAccountFirst
import mycelium.state.chatSections
import mycelium.state.clockTime
import mycelium.state.closedNotice
import mycelium.state.groupChoiceOf
import mycelium.state.groupScope
import mycelium.state.hoursNotice
import mycelium.state.isChat
import mycelium.state.isLaptop
import mycelium.state.isSubagent
import mycelium.state.isWorktree
import mycelium.state.needsInputIn
import mycelium.state.pinnedChats
import mycelium.state.pinnedConversations
import mycelium.state.projectName
import mycelium.state.resetBefore
import mycelium.state.sidebarAssistants
import mycelium.state.sidebarClosedLine
import mycelium.state.sidebarRows
import mycelium.state.sidebarSections
import mycelium.state.statusSectionName
import mycelium.state.untilEarliestOpening
import mycelium.state.workingBehindClosedDoors

sealed interface Route {
    data object Empty : Route

    data class Chat(val id: String) : Route

    data object New : Route

    data object NewChat : Route

    data class NewIn(val projectId: String) : Route

    data object Nodes : Route

    data class Node(val id: String) : Route

    data object Tasks : Route

    data object Workflows : Route

    data class Workflow(val id: String) : Route

    data object Schedules : Route

    data object Wellness : Route

    data object Projects : Route

    data object Issues : Route

    data object Archive : Route

    data object Apps : Route

    data class App(val uuid: String, val tab: String = AppOverviewTab, val deployment: String? = null) : Route

    data object Artifacts : Route

    data class Artefact(val id: String) : Route

    data object Activity : Route

    data object UsageHistory : Route

    data object Assistants : Route

    data class Assistant(val id: String) : Route

    data object NewAssistant : Route

    data class AssistantSettings(val id: String, val section: AssistantSection = AssistantSection.Identity) : Route

    data object Settings : Route
}

@Composable
fun Shell(store: Store, platform: Platform, me: Me, onSignOut: () -> Unit, initialRoute: Route = Route.Empty) {
    val wellness by store.wellness.state.collectAsState()
    val onBreak = (wellness.clock.phase == BreakPhase.OnBreak && wellness.breakEndsAt != null) || (wellness.clock.phase == BreakPhase.Noting && wellness.ended != null)

    CompositionLocalProvider(LocalAttachmentBytes provides store::attachmentBytes) {
        Box(Modifier.noticingInput(store.wellness).voiceShortcut(store.voice).focusProperties { onEnter = { if (onBreak) cancelFocusChange() } }.focusGroup().then(if (onBreak) Modifier.clearAndSetSemantics {} else Modifier)) {
            ShellFor(store, platform, me, onSignOut, initialRoute)
        }

        BreakScreen(store, platform)
    }
}

@Composable
private fun ShellFor(store: Store, platform: Platform, me: Me, onSignOut: () -> Unit, initialRoute: Route) {
    LaunchedEffect(Unit) {
        platform.timeZone()?.let { zone -> store.act { store.setTimeZone(zone) } }
    }

    if (LocalTouch.current) {
        PhoneShell(store, platform, me, onSignOut, initialRoute)

        return
    }

    val state by store.state.collectAsState()
    val problem by store.problem.collectAsState()
    val retryable by store.retryable.collectAsState()
    val hours = rememberHoursView(store)
    val groupFilter = rememberGroupFilter(platform, state, hours)
    val shownScope = groupScope(groupFilter.value, hours.closedGroupIds)
    var openRequest by remember { mutableStateOf<OpenRequest?>(null) }
    var route by remember { mutableStateOf(initialRoute) }

    LaunchedEffect(store) {
        store.openRequests.collect { route = Route.Chat(it) }
    }

    SideEffect {
        store.viewing = (route as? Route.Chat)?.id ?: (route as? Route.Assistant)?.let { state.assistants[it.id]?.conversationId }
        store.wellness.presence = activityTargetOf(route, state)
    }
    val colors = MyTheme.colors
    var sidebarWidth by remember { mutableStateOf(platform.loadPreference(SidebarWidthPreference)?.toFloatOrNull()?.dp ?: Tokens.Size.Sidebar) }
    val density = LocalDensity.current

    fun resize(width: Dp) {
        sidebarWidth = width.coerceIn(Tokens.Size.SidebarMin, Tokens.Size.SidebarMax)
        platform.savePreference(SidebarWidthPreference, sidebarWidth.value.toString())
    }

    val demand = remember { PanelDemand() }
    var overlayOpen by remember { mutableStateOf(false) }
    val overlayFocus = remember { FocusRequester() }

    BoxWithConstraints(Modifier.fillMaxSize().background(colors.bgContent)) {
        val docked = sidebarDocks(maxWidth, sidebarWidth, demand.width)

        LaunchedEffect(docked) {
            if (docked) {
                overlayOpen = false
            }
        }

        LaunchedEffect(overlayOpen) {
            if (overlayOpen) {
                runCatching { overlayFocus.requestFocus() }
            }
        }

        Row(Modifier.fillMaxSize()) {
            if (docked) {
                CompositionLocalProvider(LocalPlatform provides platform) {
                    Sidebar(
                        store = store,
                        platform = platform,
                        state = state,
                        route = route,
                        onRoute = { route = it },
                        groupFilter = groupFilter,
                        hours = hours,
                        onOpenOutOfHours = { openRequest = OpenRequest(it) },
                        modifier = Modifier.width(sidebarWidth).fillMaxHeight(),
                    )
                }

                ResizeHandle("Resize the sidebar") { moved -> resize(sidebarWidth + moved) }
            }

            Column(Modifier.weight(1f).fillMaxHeight()) {
                if (!state.connected) {
                    ReconnectingBanner()
                }

                problem?.let { ErrorBanner(it, onDismiss = store::dismissProblem, onRetry = if (retryable != null) store::retryProblem else null) }

                Box(Modifier.weight(1f).fillMaxWidth()) {
                    CompositionLocalProvider(LocalOpenList provides (if (docked) null else ({ overlayOpen = true })), LocalPanelDemand provides demand) {
                        RouteContent(store, state, platform, me, route, shownScope, hours, groupFilter.choose, onOpenOutOfHours = { openRequest = OpenRequest(it) }, onRoute = { route = it }, onSignOut = onSignOut)
                    }
                }
            }
        }

        if (!docked && overlayOpen) {
            Box(
                Modifier
                    .fillMaxSize()
                    .background(Color.Black.copy(alpha = 0.32f))
                    .clickable(interactionSource = remember { MutableInteractionSource() }, indication = null, role = Role.Button, onClickLabel = "Hide the sidebar") { overlayOpen = false },
            )

            CompositionLocalProvider(LocalPlatform provides platform) {
                Sidebar(
                    store = store,
                    platform = platform,
                    state = state,
                    route = route,
                    onRoute = { route = it; overlayOpen = false },
                    groupFilter = groupFilter,
                    hours = hours,
                    onOpenOutOfHours = { openRequest = OpenRequest(it) },
                    modifier = Modifier
                        .width(sidebarWidth)
                        .fillMaxHeight()
                        .focusRequester(overlayFocus)
                        .focusable()
                        .onPreviewKeyEvent { event ->
                            if (event.type == KeyEventType.KeyDown && event.key == Key.Escape) {
                                overlayOpen = false
                                true
                            } else {
                                false
                            }
                        },
                )
            }
        }
    }

    OpenOutOfHoursHost(store, platform, hours, openRequest, onDismiss = { openRequest = null }, onOpened = groupFilter.choose)
}

@OptIn(ExperimentalComposeUiApi::class)
@Composable
fun PhoneShell(store: Store, platform: Platform, me: Me, onSignOut: () -> Unit, initialRoute: Route = Route.Empty) {
    val state by store.state.collectAsState()
    val problem by store.problem.collectAsState()
    val retryable by store.retryable.collectAsState()
    val hours = rememberHoursView(store)
    val groupFilter = rememberGroupFilter(platform, state, hours)
    val shownScope = groupScope(groupFilter.value, hours.closedGroupIds)
    var openRequest by remember { mutableStateOf<OpenRequest?>(null) }
    var route by remember { mutableStateOf(initialRoute) }

    LaunchedEffect(store) {
        store.openRequests.collect { route = Route.Chat(it) }
    }

    SideEffect {
        store.viewing = (route as? Route.Chat)?.id ?: (route as? Route.Assistant)?.let { state.assistants[it.id]?.conversationId }
        store.wellness.presence = activityTargetOf(route, state)
    }
    var drawerOpen by remember { mutableStateOf(false) }
    val progress = remember { Animatable(0f) }
    val scope = rememberCoroutineScope()
    val reducedMotion = LocalReducedMotion.current
    val drawerShown by remember { derivedStateOf { progress.value > 0f } }
    val density = LocalDensity.current

    suspend fun settle(open: Boolean) {
        drawerOpen = open
        val target = if (open) 1f else 0f
        if (reducedMotion) progress.snapTo(target) else progress.animateTo(target)
    }

    LaunchedEffect(drawerOpen) { settle(drawerOpen) }

    BackHandler(enabled = drawerOpen) { drawerOpen = false }

    BoxWithConstraints(Modifier.fillMaxSize().background(MyTheme.colors.bgContent)) {
        val drawerWidth = minOf(Tokens.Size.Sidebar + Tokens.Space.S8 * 2, maxWidth * 0.86f)
        val drawerWidthPx = with(density) { drawerWidth.toPx() }
        fun dragBy(delta: Float) {
            scope.launch { progress.snapTo((progress.value + delta / drawerWidthPx).coerceIn(0f, 1f)) }
        }

        suspend fun release(velocity: Float) {
            settle(velocity > Tokens.Size.DrawerFlingVelocity || (velocity > -Tokens.Size.DrawerFlingVelocity && progress.value > 0.5f))
        }

        val drag = Modifier.draggable(
            state = rememberDraggableState { delta -> dragBy(delta) },
            orientation = Orientation.Horizontal,
            onDragStopped = { velocity -> release(velocity) },
        )
        val openDrag = Modifier.pointerInput(drawerWidthPx) {
            awaitEachGesture {
                val down = awaitFirstDown(requireUnconsumed = false)
                val tracker = VelocityTracker()
                var travelled = Offset.Zero
                var dragging = false

                tracker.addPosition(down.uptimeMillis, down.position)

                while (true) {
                    val event = awaitPointerEvent()
                    val change = event.changes.firstOrNull { it.id == down.id } ?: return@awaitEachGesture

                    if (!change.pressed) {
                        break
                    }

                    if (!dragging && change.isConsumed) {
                        return@awaitEachGesture
                    }

                    if (!dragging) {
                        travelled += change.positionChange()

                        if (travelled.getDistance() < viewConfiguration.touchSlop) {
                            continue
                        }

                        if (travelled.x <= 0f || travelled.x < travelled.y.absoluteValue * 2f) {
                            return@awaitEachGesture
                        }

                        dragging = true
                        dragBy(travelled.x)
                        tracker.addPosition(change.uptimeMillis, change.position)
                        change.consume()
                        continue
                    }

                    tracker.addPosition(change.uptimeMillis, change.position)
                    dragBy(change.positionChange().x)
                    change.consume()
                }

                if (dragging) {
                    scope.launch { release(tracker.calculateVelocity().x) }
                }
            }
        }

        CompositionLocalProvider(LocalOpenList provides { drawerOpen = true }) {
            Column(Modifier.fillMaxSize()) {
                if (!state.connected) {
                    ReconnectingBanner()
                }

                problem?.let { ErrorBanner(it, onDismiss = store::dismissProblem, onRetry = if (retryable != null) store::retryProblem else null) }

                Box(Modifier.weight(1f).fillMaxWidth().then(if (route is Route.Artefact) Modifier else openDrag)) {
                    RouteContent(store, state, platform, me, route, shownScope, hours, groupFilter.choose, onOpenOutOfHours = { openRequest = OpenRequest(it) }, onRoute = { route = it }, onSignOut = onSignOut)
                }
            }
        }

        if (drawerShown) {
            Box(
                Modifier
                    .fillMaxSize()
                    .graphicsLayer { alpha = progress.value }
                    .background(Color.Black.copy(alpha = 0.32f))
                    .then(drag)
                    .clickable(interactionSource = remember { MutableInteractionSource() }, indication = null, role = Role.Button, onClickLabel = "Close conversations") { drawerOpen = false },
            )

            CompositionLocalProvider(LocalPlatform provides platform) {
                Sidebar(
                    store = store,
                    platform = platform,
                    state = state,
                    route = route,
                    onRoute = { route = it; drawerOpen = false },
                    groupFilter = groupFilter,
                    hours = hours,
                    onOpenOutOfHours = { openRequest = OpenRequest(it) },
                    modifier = Modifier
                        .width(drawerWidth)
                        .fillMaxHeight()
                        .offset { IntOffset((-(1f - progress.value) * drawerWidthPx).roundToInt(), 0) }
                        .then(drag),
                )
            }
        }
    }

    OpenOutOfHoursHost(store, platform, hours, openRequest, onDismiss = { openRequest = null }, onOpened = groupFilter.choose)
}

@Composable
private fun RouteContent(store: Store, state: AppState, platform: Platform, me: Me, route: Route, groupFilter: String, hours: HoursView, onChooseGroup: (String) -> Unit, onOpenOutOfHours: (String?) -> Unit, onRoute: (Route) -> Unit, onSignOut: () -> Unit) {
    var settingsStart by remember { mutableStateOf<SettingsSection?>(null) }
    var settingsGroup by remember { mutableStateOf<String?>(null) }
    var settingsGroupTab by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(store) {
        store.screenRequests.collect { path ->
            val target = screenTarget(path) ?: return@collect

            settingsStart = target.second
            onRoute(target.first)
        }
    }

    LaunchedEffect(route) {
        if (route != Route.Settings) {
            settingsStart = null
            settingsGroup = null
            settingsGroupTab = null
        }
    }

    val reviewInstructions: (String?) -> Unit = { layer ->
        val conversation = (route as? Route.Chat)?.let { state.conversations[it.id] }
        val group = reviewedGroup(layer, state.groups.values, conversation?.groupId)

        settingsStart = if (group != null) SettingsSection.Groups else SettingsSection.Instructions
        settingsGroup = group?.id
        settingsGroupTab = if (group != null) GroupInstructionsTab else null
        onRoute(Route.Settings)
    }

    val ides = if (platform.opensIdes) remember(platform) { IdeLauncher(store, platform) { settingsStart = SettingsSection.Ides; onRoute(Route.Settings) } } else null
    val appDeployments = remember(store) { store.api::appDeployment }

    LaunchedEffect(store, state.integrationsRevision) {
        attempt { store.refreshChatToolServers() }
    }

    CompositionLocalProvider(LocalPlatform provides platform, LocalQuestionAnswers provides questionAnswers(store, state), LocalBrowserTasks provides browserTasks(store, state), LocalMessageProposals provides messageProposals(store, state), LocalStartProposals provides startProposals(store, state), LocalApiAccessRequests provides apiAccessRequests(store, state), LocalArtifactAccessControls provides ArtifactAccessControls(store, state), LocalSshAccessRequests provides sshAccessRequests(store, state), LocalArtifactOpener provides artifactOpener(state) { onRoute(Route.Artefact(it)) }, LocalReviewInstructions provides reviewInstructions, LocalGroupFilter provides groupFilter, LocalHoursView provides hours, LocalIdes provides ides, LocalAppsOpener provides { onRoute(Route.Apps) }, LocalAppOpener provides { uuid, deployment -> onRoute(Route.App(uuid, if (deployment != null) AppLogsTab else AppOverviewTab, deployment)) }, LocalAppDeployments provides appDeployments) {
        val selected = routeGroup(route, state, groupFilter)
        val closed = closedNotice(hours, selected, workingBehindClosedDoors(state.conversations.values, hours, selected), platform).takeIf { route.opensBehindHours() }
        val notice = hoursNotice(hours, groupChoiceOf(groupFilter), platform)

        Column(Modifier.fillMaxSize()) {
            notice?.let { shown ->
                HoursBanner(shown) {
                    if (shown.action == HoursAction.CloseNow) {
                        store.act { store.closeGroupOpening(shown.groupId) }
                    } else {
                        settingsStart = SettingsSection.Groups
                        settingsGroup = shown.groupId
                        settingsGroupTab = GroupHoursTab
                        onRoute(Route.Settings)
                    }
                }
            }

            Box(Modifier.weight(1f).fillMaxWidth()) {
                if (closed != null) {
                    ClosedScreen(closed, onSelect = onChooseGroup, onOpenOutOfHours = { onOpenOutOfHours(closed.groupId) })
                } else when (route) {
                    Route.Empty -> EmptyMain(state, onNew = { onRoute(Route.New) })
                    is Route.Chat -> ConversationScreen(store, state, route.id, onArchived = { onRoute(Route.Empty) }, onOpenNodes = { onRoute(Route.Nodes) }, onOpenNode = { onRoute(Route.Node(it)) }, onOpenConversation = { onRoute(Route.Chat(it)) }, onOpenArtefact = { onRoute(Route.Artefact(it)) }, onOpenWorkflow = { onRoute(Route.Workflow(it)) })
                    Route.New -> NewConversationScreen(store, state, onCreated = { onRoute(Route.Chat(it.id)) })
                    Route.NewChat -> NewChatScreen(store, onCreated = { onRoute(Route.Chat(it.id)) }, onOpenSettings = { onRoute(Route.Settings) }, onOpenNodes = { onRoute(Route.Nodes) })
                    is Route.NewIn -> NewConversationScreen(store, state, onCreated = { onRoute(Route.Chat(it.id)) }, projectId = route.projectId)
                    Route.Nodes -> NodesScreen(store, state, platform, onOpenNode = { onRoute(Route.Node(it)) })
                    is Route.Node -> NodePage(store, state, route.id, platform, onBack = { onRoute(Route.Nodes) }, onOpenConversation = { onRoute(Route.Chat(it)) })
                    Route.Tasks -> TasksScreen(store, state, platform, onOpen = { onRoute(Route.Chat(it)) })
                    Route.Workflows -> WorkflowsScreen(store, state, onOpenConversation = { onRoute(Route.Chat(it)) }, onOpenWorkflow = { onRoute(Route.Workflow(it)) })
                    is Route.Workflow -> WorkflowScreen(store, state, route.id, onBack = { onRoute(Route.Workflows) }, onOpenConversation = { onRoute(Route.Chat(it)) }, onOpenWorkflow = { onRoute(Route.Workflow(it)) }, onOpenProjects = { onRoute(Route.Projects) })
                    Route.Schedules -> SchedulesScreen(store, state)
                    Route.Wellness -> WellnessScreen(store, platform, onOpenAssistant = { onRoute(Route.Assistant(it)) }, onOpenConversation = { onRoute(Route.Chat(it)) })
                    Route.Projects -> ProjectsScreen(store, state, onNewConversation = { onRoute(Route.NewIn(it)) })
                    Route.Issues -> IssuesScreen(store, state, onOpened = { onRoute(Route.Chat(it.id)) })
                    Route.Archive -> ArchiveScreen(store, state, onOpen = { onRoute(Route.Chat(it)) })
                    Route.Apps -> AppsScreen(store, state, platform, onOpenApp = { onRoute(Route.App(it)) }, onOpenConversation = { onRoute(Route.Chat(it)) }, onNewConversation = { onRoute(Route.New) })
                    is Route.App -> AppPage(store, state, platform, route.uuid, route.tab, route.deployment, onBack = { onRoute(Route.Apps) }, onOpenConversation = { onRoute(Route.Chat(it)) })
                    Route.Artifacts -> ArtifactsScreen(store, state, me, platform, onOpen = { onRoute(Route.Artefact(it)) })
                    is Route.Artefact -> ArtefactFullView(store, state, platform, route.id, onBack = { onRoute(Route.Artifacts) }, onOpenNodes = { onRoute(Route.Nodes) }, onOpenConversation = { onRoute(Route.Chat(it)) }, onOpenProjects = { onRoute(Route.Projects) })
                    Route.Assistants, Route.NewAssistant -> AssistantsScreen(store, state, onOpen = { onRoute(Route.Assistant(it.id)) }, onSettings = { onRoute(Route.AssistantSettings(it.id)) }, onNew = { onRoute(Route.NewAssistant) }, creating = route == Route.NewAssistant, onCloseNew = { onRoute(Route.Assistants) })
                    is Route.Assistant -> AssistantScreen(store, state, route.id, onOpenSettings = { onRoute(Route.AssistantSettings(route.id)) }, onManageActions = { onRoute(Route.AssistantSettings(route.id, AssistantSection.SchedulesAndActions)) }, onBack = { onRoute(Route.Assistants) }, onOpenConversation = { onRoute(Route.Chat(it)) }, onOpenArtefact = { onRoute(Route.Artefact(it)) })
                    is Route.AssistantSettings -> AssistantSettingsScreen(store, state, route.id, route.section, onDone = { onRoute(Route.Assistants) }, onBack = { onRoute(Route.Assistant(route.id)) })
                    Route.Activity -> ActivityScreen(store, state, onOpenConversation = { onRoute(Route.Chat(it)) })
                    Route.UsageHistory -> UsageHistoryScreen(store)
                    Route.Settings -> CompositionLocalProvider(LocalOpenConversation provides { onRoute(Route.Chat(it)) }) { SettingsScreen(store, me, store.api.baseUrl, startOn = settingsStart, startGroup = settingsGroup, startGroupTab = settingsGroupTab, onSignOut = onSignOut) }
                }
            }
        }

        WellnessPresence(store, platform)
    }
}

internal fun screenTarget(path: String): Pair<Route, SettingsSection?>? {
    val parts = path.trim('/').split('/')
    val argument = parts.getOrNull(1)

    return when (parts[0]) {
        "settings" -> Route.Settings to argument?.let { key -> SettingsSection.entries.firstOrNull { settingsKey(it) == key } }
        "chat" -> argument?.let { Route.Chat(it) to null }
        "assistant" -> argument?.let { Route.Assistant(it) to null }
        "assistants" -> Route.Assistants to null
        "new" -> Route.New to null
        "nodes" -> Route.Nodes to null
        "tasks" -> Route.Tasks to null
        "workflows" -> Route.Workflows to null
        "schedules" -> Route.Schedules to null
        "wellness" -> Route.Wellness to null
        "projects" -> Route.Projects to null
        "issues" -> Route.Issues to null
        "archive" -> Route.Archive to null
        "artifacts" -> Route.Artifacts to null
        "activity" -> Route.Activity to null
        "usage" -> Route.UsageHistory to null
        else -> null
    }
}

internal fun settingsKey(section: SettingsSection): String = section.label.lowercase().replace(' ', '-')

private fun Route.opensBehindHours(): Boolean = this !in HoursExemptRoutes && this !is Route.Node

private val HoursExemptRoutes = setOf(Route.Settings, Route.Wellness, Route.Activity, Route.UsageHistory, Route.Nodes)

private fun routeGroup(route: Route, state: AppState, selected: String): String = when (route) {
    is Route.Chat -> state.conversations[route.id]?.groupId ?: NoGroup
    is Route.Assistant -> state.assistants[route.id]?.groupId ?: NoGroup
    Route.NewChat -> NoGroup
    else -> groupChoiceOf(selected)
}

@Composable
private fun EmptyMain(state: AppState, onNew: () -> Unit) {
    val hasNode = state.nodes.values.any { it.online }

    Column(Modifier.fillMaxSize()) {
        if (LocalOpenList.current != null) {
            Bar {
                BarTitle("Mycelium")
            }
        }

        Column(
            Modifier.weight(1f).fillMaxWidth().padding(horizontal = Tokens.Space.S4),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.Center,
        ) {
            Label(
                if (hasNode) "Choose a conversation or start a new one." else "No node is online. Join a node from Nodes, then start a conversation.",
                color = MyTheme.colors.textMuted,
            )

            if (hasNode) {
                MyButton(onNew, text = "New conversation", icon = MyIcon.Plus, variant = ButtonVariant.Outline, modifier = Modifier.padding(top = Tokens.Space.S3))
            }
        }
    }
}

private const val WorkspacesMode = "Workspaces"
private const val ChatsMode = "Chats"

@Composable
private fun Sidebar(store: Store, platform: Platform, state: AppState, route: Route, onRoute: (Route) -> Unit, groupFilter: GroupFilterChoice, hours: HoursView, onOpenOutOfHours: (String?) -> Unit, modifier: Modifier = Modifier) {
    val colors = MyTheme.colors
    var settledOpen by remember { mutableStateOf(platform.loadPreference(SettledOpenPreference) == true.toString()) }
    val scope = groupScope(groupFilter.value, hours.closedGroupIds)
    val closedNotice = closedNotice(hours, groupFilter.value, 0, platform)
    val closedLine = sidebarClosedLine(hours, groupFilter.value, platform)
    val everythingClosed = closedNotice != null && closedNotice.groupId == null
    val appClosed = !hours.appOpen
    val workspaceConversations = if (appClosed) state.conversations.values.filter { it.groupId != null } else state.conversations.values
    val pinned = pinnedConversations(workspaceConversations, scope)
    val sections = sidebarSections(workspaceConversations, scope)
    val openChats = if (appClosed) emptyList() else state.conversations.values.filterNot { hours.closed(it.groupId) }
    val pinnedChatRows = pinnedChats(openChats)
    val chats = chatSections(openChats)

    LaunchedEffect(Unit) {
        store.load { store.refreshGroups() }
        store.load { store.refreshProjects() }
    }

    LaunchedEffect(Unit) {
        store.load { store.refreshAssistants() }
    }
    var collapsed by remember { mutableStateOf(emptySet<String>()) }
    var usageOpen by remember { mutableStateOf(platform.loadPreference(UsageOpenPreference) == true.toString()) }
    var providerUsage by remember(store) { mutableStateOf(emptyList<ModelProviderUsage>()) }
    val voice by store.voice.state.collectAsState()
    val voiceAssistant = state.assistants.values.firstOrNull { it.builtinKey == VoiceAssistantKey }

    LaunchedEffect(voiceAssistant?.id) {
        store.voice.onOpenThread { voiceAssistant?.let { onRoute(Route.Assistant(it.id)) } }
    }

    LaunchedEffect(store, usageOpen) {
        if (usageOpen) {
            while (true) {
                attempt {
                    providerUsage = store.api.modelProviderUsage()
                }

                delay(MinuteMillis)
            }
        }
    }

    var mode by remember { mutableStateOf(if (route == Route.NewChat || (route as? Route.Chat)?.let { state.conversations[it.id]?.isChat } == true) ChatsMode else WorkspacesMode) }
    var lastOpened by remember { mutableStateOf(mapOf(WorkspacesMode to Route.New, ChatsMode to Route.NewChat)) }
    val openedMode = when (route) {
        Route.NewChat -> ChatsMode
        is Route.Chat -> state.conversations[route.id]?.let { if (it.isChat) ChatsMode else WorkspacesMode }
        Route.Settings, Route.Activity, Route.UsageHistory, Route.Empty -> null
        Route.Assistants, is Route.Assistant, Route.NewAssistant, is Route.AssistantSettings -> null
        else -> WorkspacesMode
    }

    LaunchedEffect(route, openedMode) {
        if (openedMode != null) {
            mode = openedMode
            lastOpened = lastOpened + (openedMode to route)
        }
    }

    Column(modifier.background(colors.bgPanel)) {
        Segmented(listOf(WorkspacesMode, ChatsMode), mode, { chosen -> mode = chosen; lastOpened[chosen]?.let(onRoute) }, groupLabel = "Sidebar mode", modifier = Modifier.fillMaxWidth().padding(Tokens.Space.S1_5), stretch = true)

        if (mode == WorkspacesMode && state.groups.isNotEmpty()) {
            val openGroups = state.groups.values.filter { it.id !in hours.closedGroupIds }
            val choices = listOf(Choice(AllGroups, "All groups")) +
                openGroups.sortedBy { it.name.lowercase() }.map { group ->
                    val waiting = if (group.id == groupFilter.value) 0 else needsInputIn(state.conversations.values, group.id)

                    Choice(group.id, group.name, if (waiting > 0) "$waiting needs input" else null)
                } +
                Choice(NoGroup, "No group")
            val closedCount = hours.closedGroupIds.size

            Box(Modifier.fillMaxWidth().padding(start = Tokens.Space.S1_5, end = Tokens.Space.S1_5, bottom = Tokens.Space.S1_5)) {
                OptionMenu(
                    "Group",
                    choices,
                    groupFilter.value,
                    groupFilter.choose,
                    title = "Show",
                    icon = MyIcon.Layers,
                    fill = true,
                    label = state.groups[groupFilter.value]?.name,
                    footer = if (closedCount == 0) null else OptionFooter("Open a closed group", "$closedCount closed${untilEarliestOpening(hours, platform)?.let { " until $it" }.orEmpty()}", MyIcon.Moon) { onOpenOutOfHours(null) },
                )
            }
        }

        Hairline()

        Column(Modifier.fillMaxWidth().padding(Tokens.Space.S1_5)) {
            if (mode == ChatsMode) {
                if (!appClosed) {
                    SidebarLink("New chat", MyIcon.Plus, selected = route == Route.NewChat) { onRoute(Route.NewChat) }
                }
            } else if (everythingClosed) {
                SidebarLink("Wellness & Learning", MyIcon.Leaf, selected = route == Route.Wellness, tint = MyTheme.colors.leaf) { onRoute(Route.Wellness) }
            } else {
                SidebarLink("New conversation", MyIcon.Plus, selected = route == Route.New) { onRoute(Route.New) }
                SidebarLink("History", MyIcon.Clock, selected = route == Route.Archive) { onRoute(Route.Archive) }
                if (appsLinkVisible(state.chatToolServers)) {
                    SidebarLink("Apps", MyIcon.AppWindow, selected = route == Route.Apps || route is Route.App) { onRoute(Route.Apps) }
                }

                SidebarLink("Artefacts", MyIcon.Layers, selected = route == Route.Artifacts || route is Route.Artefact) { onRoute(Route.Artifacts) }

                SidebarLink("Tasks", MyIcon.List, selected = route == Route.Tasks) { onRoute(Route.Tasks) }
                SidebarLink("Workflows", MyIcon.Workflow, selected = route == Route.Workflows || route is Route.Workflow) { onRoute(Route.Workflows) }

                SidebarLink("Schedules", MyIcon.Calendar, selected = route == Route.Schedules) { onRoute(Route.Schedules) }
                SidebarLink("Wellness & Learning", MyIcon.Leaf, selected = route == Route.Wellness, tint = MyTheme.colors.leaf) { onRoute(Route.Wellness) }
            }
        }

        Hairline()

        val list = rememberLazyListState()

        LazyColumn(Modifier.weight(1f).fillMaxWidth().scrollbar(list), list, contentPadding = PaddingValues(horizontal = Tokens.Space.S1_5, vertical = Tokens.Space.S2)) {
            if (!everythingClosed) {
                sidebarAssistantItems(store, state, sidebarAssistants(state.assistants.values, scope), route, onRoute)
            }

            if (mode == ChatsMode) {
                if (appClosed && closedLine != null) {
                    item(key = "closed") {
                        Label(closedLine, Modifier.padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1), Tokens.Type.Meta, colors.textMuted)
                    }
                }

                conversationRows(PinnedName, pinnedChatRows, state, store, route, onRoute, collapsed, { collapsed = it }, withNode = false)

                for (section in chats) {
                    conversationRows(section.name, section.rows, state, store, route, onRoute, collapsed, { collapsed = it }, withNode = false)
                }
            } else {
                item(key = "label") {
                    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                        Label("Conversations", Modifier.weight(1f).padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1), Tokens.Type.Meta, colors.textMuted)
                    }
                }

                if (closedLine != null) {
                    item(key = "closed") {
                        Label(closedLine, Modifier.padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1), Tokens.Type.Meta, colors.textMuted)
                    }
                }

                conversationRows(PinnedName, pinned, state, store, route, onRoute, collapsed, { collapsed = it }, withNode = true)

                for (section in sections) {
                    val settled = section.status == ConversationStatus.Done

                    item(key = "section-${section.status}") {
                        SectionHeading(section, open = settledOpen.takeIf { settled }, onToggle = { settledOpen = !settledOpen; platform.savePreference(SettledOpenPreference, settledOpen.toString()) })
                    }

                    if (!settled || settledOpen) {
                        conversationRows(null, section.rows, state, store, route, onRoute, collapsed, { collapsed = it }, withNode = true)
                    }
                }
            }
        }

        if (usageOpen) {
            val current = (route as? Route.Chat)?.let { state.conversations[it.id] }

            UsageCard(chatAccountFirst(agentAccounts(state.nodes.values), current?.agent, current?.let { state.nodes[it.nodeId]?.name }, current?.account), Modifier.padding(Tokens.Space.S1_5), providerUsage, current?.providerId, onOpenHistory = { onRoute(Route.UsageHistory) }, onUseReset = { account, key -> store.api.useReset(UseResetRequest(account.agent, account.label.takeUnless { it == UnnamedAccount }, key)).outcome })
        }

        if (voice.open && voice.privacy.voiceEnabled) {
            VoiceCard(
                voice,
                VoiceCardActions(
                    onStopListening = store.voice::stopListening,
                    onCancel = store.voice::cancelListening,
                    onStopTurn = store.voice::stopTurn,
                    onStopSpeaking = store.voice::stopSpeaking,
                    onTalkAgain = store.voice::startListening,
                    onTryAgain = store.voice::startListening,
                    onTypeInstead = store.voice::typeInstead,
                    onOpenConversation = store.voice::openThread,
                    onOpenProviders = { store.voice.close(); store.openScreen("settings/providers") },
                    onOpenAndroidSettings = platform.takeIf { it.opensMicrophoneSettings }?.let { opener -> { opener.openMicrophoneSettings() } },
                    onClose = store.voice::close,
                ),
                Modifier.padding(Tokens.Space.S1_5),
            )
        }

        Hairline()

        BoxWithConstraints(Modifier.fillMaxWidth()) {
            val labelled = mode != WorkspacesMode || maxWidth >= FooterLabelsWidth

            Row(
                Modifier.fillMaxWidth().heightIn(min = barHeight()).padding(horizontal = Tokens.Space.S1),
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                val appearance = LocalAppearance.current

                if (mode == WorkspacesMode) {
                    MyButton({ onRoute(Route.Nodes) }, text = "Nodes".takeIf { labelled }, icon = MyIcon.Server, description = "Nodes", muted = true, selected = route == Route.Nodes || route is Route.Node)
                    MyButton({ onRoute(Route.Projects) }, text = "Projects".takeIf { labelled }, icon = MyIcon.Folder, description = "Projects", muted = true, selected = route == Route.Projects)
                    MyButton({ onRoute(Route.Issues) }, text = "Issues".takeIf { labelled }, icon = MyIcon.Issue, description = "Issues", muted = true, selected = route == Route.Issues)
                }

                Box(Modifier.weight(1f))
                if (voice.privacy.voiceEnabled) {
                    val listening = voice.phase == VoicePhase.Listening
                    val wakeListening = voice.device.wakePhraseEnabled && platform.hearsWakePhrase
                    val name = when {
                        listening -> "Stop listening"
                        voice.open -> "Close Voice"
                        wakeListening -> "Talk to Voice, listening for ${wakePhraseName(voice.device.wakePhrase)}"
                        else -> "Talk to Voice"
                    }

                    MyButton(
                        {
                            when {
                                listening -> store.voice.stopListening()
                                voice.open -> store.voice.close()
                                else -> {
                                    usageOpen = false
                                    platform.savePreference(UsageOpenPreference, false.toString())
                                    store.voice.startListening()
                                }
                            }
                        },
                        icon = MyIcon.Microphone,
                        description = name,
                        muted = !listening,
                        selected = voice.open,
                        tooltip = if (voice.open) name else "$name ($VoiceShortcutLabel)",
                    )
                }

                MyButton({ usageOpen = !usageOpen; platform.savePreference(UsageOpenPreference, usageOpen.toString()); if (usageOpen) store.voice.close() }, icon = MyIcon.Gauge, description = "Usage", muted = true, selected = usageOpen || route == Route.UsageHistory)
                MyButton(appearance.onToggleDark, icon = if (appearance.dark) MyIcon.Sun else MyIcon.Moon, description = if (appearance.dark) "Switch to light mode" else "Switch to dark mode", muted = true)
                MyButton({ onRoute(Route.Activity) }, icon = MyIcon.Activity, description = "Activity", muted = true, selected = route == Route.Activity)
                MyButton({ onRoute(Route.Settings) }, icon = MyIcon.Settings, description = "Settings", muted = true, selected = route == Route.Settings)
            }
        }
    }
}

private fun LazyListScope.conversationRows(
    title: String?,
    parents: List<Conversation>,
    state: AppState,
    store: Store,
    route: Route,
    onRoute: (Route) -> Unit,
    collapsed: Set<String>,
    onCollapsed: (Set<String>) -> Unit,
    withNode: Boolean,
) {
    if (parents.isEmpty()) {
        return
    }

    if (title != null) {
        item(key = "heading-$title") {
            if (title == PinnedName) {
                Row(
                    Modifier
                        .fillMaxWidth()
                        .padding(top = Tokens.Space.S2)
                        .heightIn(min = rowHeight())
                        .padding(horizontal = Tokens.Space.S2),
                    horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Icon(MyIcon.Pin, MyTheme.colors.textMuted, Tokens.Size.IconMd)
                    Label(title, Modifier.weight(1f).semantics { heading() }, Tokens.Type.UiStrong, MyTheme.colors.textMuted, maxLines = 1)
                }
            } else {
                Label(title, Modifier.padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1).semantics { heading() }, Tokens.Type.Meta, MyTheme.colors.textMuted)
            }
        }
    }

    items(sidebarRows(parents, state.conversations.values, collapsed), key = { it.conversation.id }) { row ->
        ConversationRow(
            row.conversation,
            if (withNode) state.nodes[row.conversation.nodeId] else null,
            row.conversation.projectId?.let { state.projects[it] },
            selected = row.conversation.id == (route as? Route.Chat)?.id,
            onClick = { onRoute(Route.Chat(row.conversation.id)) },
            depth = row.depth,
            sideChats = row.sideChats,
            pingHeld = row.conversation.assistantId != null && store.wellness.holdsPings(LocalHoursView.current?.closed(row.conversation.groupId) == true),
            expanded = row.conversation.id !in collapsed,
            onToggle = { onCollapsed(if (row.conversation.id in collapsed) collapsed - row.conversation.id else collapsed + row.conversation.id) },
            onPin = { store.pin(row.conversation, row.conversation.pinnedAt == null) },
            onRename = { title -> store.rename(row.conversation, title) },
            onCloseSession = { store.closeSession(row.conversation) },
            onDismiss = { store.dismissFailure(row.conversation) },
            onArchive = {
                store.act { store.api.patchConversation(row.conversation.id, PatchConversationRequest(archived = true)) }

                if (row.conversation.id == (route as? Route.Chat)?.id) {
                    onRoute(if (row.conversation.isChat) Route.NewChat else Route.New)
                }
            },
            onCancelResume = { store.act { store.api.cancelResume(row.conversation.id) } },
        )
    }
}

@Composable
private fun SidebarLink(text: String, icon: MyIcon, selected: Boolean, tint: Color? = null, onClick: () -> Unit) {
    val colors = MyTheme.colors

    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = rowHeight())
            .pressable(onClick, role = androidx.compose.ui.semantics.Role.Button, selected = selected, shape = BlockShape)
            .padding(horizontal = Tokens.Space.S2),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(icon, tint ?: colors.textMuted, Tokens.Size.IconMd)
        Label(text, maxLines = 1)
    }
}

@Composable
private fun SectionHeading(section: SidebarSection, open: Boolean?, onToggle: () -> Unit) {
    val folding = if (open == null) Modifier else Modifier.pressable(onToggle, description = "${section.name}, ${section.rows.size} conversations, ${if (open) "expanded" else "collapsed"}", shape = BlockShape)

    Row(
        Modifier
            .fillMaxWidth()
            .padding(top = Tokens.Space.S2)
            .heightIn(min = rowHeight())
            .then(folding)
            .padding(horizontal = Tokens.Space.S2),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        StatusGlyph(section.status, Tokens.Size.IconMd)
        Label(if (open == null) section.name else "${section.name} (${section.rows.size})", Modifier.weight(1f).semantics { heading() }, Tokens.Type.UiStrong, MyTheme.colors.textMuted, maxLines = 1)

        if (open != null) {
            Icon(if (open) MyIcon.ChevronDown else MyIcon.ChevronRight, MyTheme.colors.textMuted, Tokens.Size.IconSm)
        }
    }
}

@Composable
private fun ConversationRow(row: Conversation, node: NodeSummary?, project: Project?, selected: Boolean, onClick: () -> Unit, depth: Int = 0, sideChats: Int = 0, pingHeld: Boolean = false, expanded: Boolean = true, onToggle: () -> Unit = {}, onPin: () -> Unit = {}, onRename: (String) -> Unit = {}, onCloseSession: () -> Unit = {}, onDismiss: () -> Unit = {}, onArchive: () -> Unit = {}, onCancelResume: () -> Unit = {}) {
    val colors = MyTheme.colors
    val platform = LocalPlatform.current
    val menu = rememberRowMenuState()
    var renaming by remember { mutableStateOf(false) }
    val pausedUntil = row.resumeAt?.let(::resetBefore)?.let { reset -> "Paused until ${clockTime(reset) { millis -> platform?.localDateTime(millis) }}" }
    val statusName = pausedUntil ?: statusSectionName(row.status)
    val notRunning = node != null && (!node.online || row.id !in node.sessions)
    val indent = Tokens.Space.S3 * depth
    val projectName = project?.takeIf { !row.isChat }?.name
    val shownTitle = projectName?.let { "$it ${row.title}" } ?: row.title
    val place = listOfNotNull(row.branch, row.pullRequestLabel).joinToString("  ")

    Box(
        Modifier
            .fillMaxWidth()
            .rowContextMenu(menu),
        contentAlignment = Alignment.CenterStart,
    ) {
        if (renaming) {
            var title by remember { mutableStateOf(row.title) }
            val save = {
                renaming = false
                onRename(title.trim())
            }

            Dialog(onDismiss = { renaming = false }) {
                Label("Rename conversation", Modifier.semantics { heading() }, Tokens.Type.Title)
                TextInput(title, { title = it }, label = "Conversation name", placeholder = "Leave empty to use the suggested title", onSubmit = save)

                FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                    MyButton({ renaming = false }, text = "Cancel", muted = true)
                    MyButton(save, text = "Save", variant = ButtonVariant.Outline)
                }
            }
        }

        Row(
            Modifier
                .fillMaxWidth()
                .pressable(
                    onClick,
                    selected = selected || menu.open,
                    shape = BlockShape,
                    description = "$shownTitle, $statusName, ${agentName(row.agent)}${node?.let { " on ${it.name}" }.orEmpty()}${if (notRunning) ", not running" else ""}${row.sideKind?.let { if (row.isSubagent) ", subagent" else ", side chat" }.orEmpty()}${if (!row.isChat && place.isNotEmpty()) ", " + listOfNotNull(row.branch?.let { "branch $it" }, row.pullRequestLabel).joinToString(", ") else ""}",
                )
                .padding(start = Tokens.Space.S2 + (if (row.isChat && sideChats == 0) 0.dp else Tokens.Space.S3) + indent, end = Tokens.Space.S2, top = Tokens.Space.S1_5, bottom = Tokens.Space.S1_5),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            val badge = when (row.status) {
                ConversationStatus.Working -> colors.statusWorking
                ConversationStatus.Input, ConversationStatus.Review -> colors.statusAttention.takeUnless { pingHeld }
                ConversationStatus.Failed -> if (row.resumeAt != null) colors.statusAttention else colors.statusFailed
                ConversationStatus.Done -> null
            }

            if (!row.isChat) {
                Box {
                    if (row.isSubagent) {
                        Icon(MyIcon.Agent, colors.textMuted, Tokens.Size.IconSm)
                    } else {
                        ProjectIcon(project, Tokens.Size.IconSm)
                    }

                    if (badge != null) {
                        val onIcon = project != null
                        val placement = if (onIcon) Modifier.align(Alignment.BottomEnd).offset(x = Tokens.Space.S0_5, y = Tokens.Space.S0_5) else Modifier.align(Alignment.Center)

                        Box(placement.size(Tokens.Space.S2).background(badge, CircleShape))
                    }
                }
            }

            Column(Modifier.weight(1f)) {
                val title = buildAnnotatedString {
                    if (projectName != null) {
                        withStyle(SpanStyle(fontWeight = FontWeight.SemiBold)) { append(projectName) }
                        append(" ")
                    }

                    append(row.title)
                }
                BasicText(title, style = Tokens.Type.Ui.themed().copy(color = if (notRunning) colors.textMuted else colors.text), maxLines = 1, overflow = TextOverflow.Ellipsis)

                if (pausedUntil != null) {
                    Label(pausedUntil, style = Tokens.Type.Meta, color = colors.statusAttention, maxLines = 1)
                }

                if (!row.isChat && place.isNotEmpty()) {
                    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
                        if (isWorktree(row, project)) {
                            Icon(MyIcon.Branch, colors.textMuted, Tokens.Size.IconSm)
                        }

                        Label(place, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
                    }
                }
            }

            if (row.resumeAt != null) {
                Box(Modifier.width(Tokens.Size.ControlMd * 2))
            } else {
                Label(timestamp(row.updatedAt), style = Tokens.Type.Meta, color = colors.textFaint, maxLines = 1)
            }

            if (row.isChat) {
                val unread = when (row.status) {
                    ConversationStatus.Input, ConversationStatus.Review -> colors.text.takeUnless { pingHeld }
                    else -> badge
                }

                Box(Modifier.size(Tokens.Space.S2).background(unread ?: Color.Transparent, CircleShape))
            } else {
                Icon(agentIcon(row.agent), colors.textMuted, Tokens.Size.IconSm)
            }

            if (node != null) {
                Icon(nodeIcon(node), nodeColour(node), Tokens.Size.IconSm)
            }
        }

        RowMenuPopup(menu) { close ->
            MenuItem(if (row.pinnedAt == null) "Pin" else "Unpin", onClick = { close(); onPin() }, leading = MyIcon.Pin)
            MenuItem("Rename", onClick = { close(); renaming = true }, leading = MyIcon.Pencil)

            if (!row.isChat) {
                MenuItem("Close session", onClick = { close(); onCloseSession() }, leading = MyIcon.Stop)
            }

            if (row.status == ConversationStatus.Failed) {
                MenuItem("Dismiss", onClick = { close(); onDismiss() }, leading = MyIcon.Check)
            }

            MenuItem("Archive", onClick = { close(); onArchive() }, leading = MyIcon.Archive)
        }

        if (row.resumeAt != null) {
            Box(Modifier.align(Alignment.CenterEnd).padding(end = Tokens.Space.S2 * 3 + Tokens.Size.IconSm * 2 + Tokens.Size.ControlSm)) {
                MyButton(onCancelResume, text = "Cancel", description = "Cancel the automatic resume of ${row.title}, due at ${clockTime(row.resumeAt) { millis -> platform?.localDateTime(millis) }}", size = ButtonSize.Sm, muted = true)
            }
        }

        if (sideChats > 0) {
            MyButton(
                onToggle,
                modifier = Modifier.padding(start = indent),
                icon = if (expanded) MyIcon.ChevronDown else MyIcon.ChevronRight,
                description = if (expanded) "Collapse side chats of ${row.title}" else "Expand side chats of ${row.title}",
                size = ButtonSize.Sm,
                muted = true,
            )
        }
    }
}

val NodeColours = Accent.entries.filter { it != Accent.System && it != Accent.Neutral }

@Composable
fun nodeColour(node: NodeSummary): Color {
    val accent = NodeColours.firstOrNull { it.name.lowercase() == node.colour } ?: NodeColours[node.name.hashCode().mod(NodeColours.size)]

    return if (LocalAppearance.current.dark) accent.dark else accent.light
}

private const val SidebarWidthPreference = "sidebar-width"

private const val UsageOpenPreference = "usage-card-open"

private val FooterLabelsWidth = 360.dp

private const val GroupFilterPreference = "conversation-group-filter"

private const val SettledOpenPreference = "settled-open"

val LocalGroupFilter = staticCompositionLocalOf { AllGroups }

private class GroupFilterChoice(val value: String, val choose: (String) -> Unit)

@Composable
private fun rememberGroupFilter(platform: Platform, state: AppState, hours: HoursView): GroupFilterChoice {
    var saved by remember { mutableStateOf(platform.loadPreference(GroupFilterPreference) ?: AllGroups) }
    var launched by remember { mutableStateOf(false) }
    val known = saved == AllGroups || saved == NoGroup || saved in state.groups

    LaunchedEffect(state.groups.isNotEmpty()) {
        if (state.groups.isNotEmpty() && !launched) {
            launched = true

            if (saved in hours.closedGroupIds) {
                saved = AllGroups
            }
        }
    }

    return GroupFilterChoice(if (known) saved else AllGroups) { chosen -> saved = chosen; platform.savePreference(GroupFilterPreference, chosen) }
}
