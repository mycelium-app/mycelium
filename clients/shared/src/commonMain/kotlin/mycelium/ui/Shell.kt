package mycelium.ui

import androidx.compose.animation.core.Animatable
import androidx.compose.foundation.layout.offset
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.unit.IntOffset
import kotlinx.coroutines.launch
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import mycelium.api.ModelProviderUsage
import mycelium.api.UseResetRequest
import kotlin.math.roundToInt
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.backhandler.BackHandler
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
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
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.foundation.text.BasicText
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import mycelium.Platform
import mycelium.api.Conversation
import mycelium.api.Me
import mycelium.api.NodeSummary
import mycelium.api.PatchConversationRequest
import mycelium.api.Project
import androidx.compose.foundation.shape.CircleShape
import mycelium.state.AllGroups
import mycelium.state.UnnamedAccount
import mycelium.state.Choice
import mycelium.state.AppState
import mycelium.state.NoGroup
import mycelium.state.PinnedName
import mycelium.state.pinnedChats
import mycelium.state.needsInputIn
import mycelium.state.pinnedConversations
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.ui.input.pointer.PointerEventType
import androidx.compose.ui.input.pointer.isSecondaryPressed
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.input.pointer.positionChange
import androidx.compose.ui.input.pointer.util.VelocityTracker
import kotlin.math.absoluteValue
import mycelium.state.SidebarSection
import mycelium.state.Store
import androidx.compose.foundation.layout.size
import androidx.compose.ui.semantics.heading
import mycelium.api.ConversationStatus
import mycelium.state.isChat
import mycelium.state.chatSections
import mycelium.state.projectName
import androidx.compose.foundation.focusable
import androidx.compose.foundation.gestures.Orientation
import androidx.compose.foundation.gestures.draggable
import androidx.compose.foundation.gestures.rememberDraggableState
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.layout.requiredWidth
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.input.pointer.pointerHoverIcon
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import mycelium.state.agentName
import mycelium.state.clockTime
import mycelium.state.isLaptop
import mycelium.state.isSubagent
import mycelium.state.agentAccounts
import mycelium.state.chatAccountFirst
import mycelium.state.sidebarRows
import mycelium.state.AllProjects
import mycelium.state.NoProject
import mycelium.state.isWorktree
import mycelium.state.sidebarAssistants
import mycelium.state.sidebarSections
import mycelium.state.statusSectionName

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
    data object Schedules : Route

    data object Projects : Route

    data object Issues : Route

    data object Archive : Route

    data object Artifacts : Route

    data class Artefact(val id: String) : Route

    data object Activity : Route

    data object Assistants : Route

    data object NewAssistant : Route

    data class AssistantSettings(val id: String) : Route

    data object Settings : Route
}

@Composable
fun Shell(store: Store, platform: Platform, me: Me, onSignOut: () -> Unit, initialRoute: Route = Route.Empty) {
    LaunchedEffect(Unit) {
        platform.timeZone()?.let { zone -> store.act { store.setTimeZone(zone) } }
    }

    if (LocalTouch.current) {
        PhoneShell(store, platform, me, onSignOut, initialRoute)

        return
    }

    val state by store.state.collectAsState()
    val problem by store.problem.collectAsState()
    val groupFilter = rememberGroupFilter(platform, state)
    var route by remember { mutableStateOf(initialRoute) }

    LaunchedEffect(store) {
        store.openRequests.collect { route = Route.Chat(it) }
    }

    SideEffect {
        store.viewing = (route as? Route.Chat)?.id
    }
    val colors = MyTheme.colors
    var sidebarWidth by remember { mutableStateOf(platform.loadPreference(SidebarWidthPreference)?.toFloatOrNull()?.dp ?: Tokens.Size.Sidebar) }
    val density = LocalDensity.current

    fun resize(width: Dp) {
        sidebarWidth = width.coerceIn(Tokens.Size.SidebarMin, Tokens.Size.SidebarMax)
        platform.savePreference(SidebarWidthPreference, sidebarWidth.value.toString())
    }

    Row(Modifier.fillMaxSize().background(colors.bgContent)) {
        CompositionLocalProvider(LocalPlatform provides platform) {
            Sidebar(
                store = store,
                platform = platform,
                state = state,
                route = route,
                onRoute = { route = it },
                groupFilter = groupFilter,
                modifier = Modifier.width(sidebarWidth).fillMaxHeight(),
            )
        }

        ResizeHandle("Resize the sidebar") { moved -> resize(sidebarWidth + moved) }

        Column(Modifier.weight(1f).fillMaxHeight()) {
            if (!state.connected) {
                ReconnectingBanner()
            }

            problem?.let { ErrorBanner(it, onDismiss = store::dismissProblem) }

            Box(Modifier.weight(1f).fillMaxWidth()) {
                RouteContent(store, state, platform, me, route, groupFilter.value, onRoute = { route = it }, onSignOut = onSignOut)
            }
        }
    }
}

@OptIn(ExperimentalComposeUiApi::class)
@Composable
fun PhoneShell(store: Store, platform: Platform, me: Me, onSignOut: () -> Unit, initialRoute: Route = Route.Empty) {
    val state by store.state.collectAsState()
    val problem by store.problem.collectAsState()
    val groupFilter = rememberGroupFilter(platform, state)
    var route by remember { mutableStateOf(initialRoute) }

    LaunchedEffect(store) {
        store.openRequests.collect { route = Route.Chat(it) }
    }

    SideEffect {
        store.viewing = (route as? Route.Chat)?.id
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

                problem?.let { ErrorBanner(it, onDismiss = store::dismissProblem) }

                Box(Modifier.weight(1f).fillMaxWidth().then(if (route is Route.Artefact) Modifier else openDrag)) {
                    RouteContent(store, state, platform, me, route, groupFilter.value, onRoute = { route = it }, onSignOut = onSignOut)
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
                    modifier = Modifier
                        .width(drawerWidth)
                        .fillMaxHeight()
                        .offset { IntOffset((-(1f - progress.value) * drawerWidthPx).roundToInt(), 0) }
                        .then(drag),
                )
            }
        }
    }
}

@Composable
private fun RouteContent(store: Store, state: AppState, platform: Platform, me: Me, route: Route, groupFilter: String, onRoute: (Route) -> Unit, onSignOut: () -> Unit) {
    var settingsStart by remember { mutableStateOf<SettingsSection?>(null) }

    LaunchedEffect(route) {
        if (route != Route.Settings) {
            settingsStart = null
        }
    }

    CompositionLocalProvider(LocalPlatform provides platform, LocalQuestionAnswers provides questionAnswers(store, state), LocalMessageProposals provides messageProposals(store, state), LocalApiAccessRequests provides apiAccessRequests(store, state), LocalArtifactAccessControls provides ArtifactAccessControls(store, state), LocalSshAccessRequests provides sshAccessRequests(store, state), LocalArtifactOpener provides artifactOpener(state) { onRoute(Route.Artefact(it)) }, LocalReviewInstructions provides { settingsStart = SettingsSection.Instructions; onRoute(Route.Settings) }, LocalGroupFilter provides groupFilter) {
        when (route) {
            Route.Empty -> EmptyMain(state, onNew = { onRoute(Route.New) })
            is Route.Chat -> ConversationScreen(store, state, route.id, onArchived = { onRoute(Route.Empty) }, onOpenNodes = { onRoute(Route.Nodes) }, onOpenConversation = { onRoute(Route.Chat(it)) }, onOpenArtefact = { onRoute(Route.Artefact(it)) })
            Route.New -> NewConversationScreen(store, state, onCreated = { onRoute(Route.Chat(it.id)) })
            Route.NewChat -> NewChatScreen(store, onCreated = { onRoute(Route.Chat(it.id)) }, onOpenSettings = { onRoute(Route.Settings) })
            is Route.NewIn -> NewConversationScreen(store, state, onCreated = { onRoute(Route.Chat(it.id)) }, projectId = route.projectId)
            Route.Nodes -> NodesScreen(store, state, platform, onOpenNode = { onRoute(Route.Node(it)) })
            is Route.Node -> NodePage(store, state, route.id, platform, onBack = { onRoute(Route.Nodes) }, onOpenConversation = { onRoute(Route.Chat(it)) })
            Route.Tasks -> TasksScreen(store, state, platform, onOpen = { onRoute(Route.Chat(it)) })
            Route.Workflows -> WorkflowsScreen(store, state, onOpenConversation = { onRoute(Route.Chat(it)) })
            Route.Schedules -> SchedulesScreen(store, state)
            Route.Projects -> ProjectsScreen(store, state, onNewConversation = { onRoute(Route.NewIn(it)) })
            Route.Issues -> IssuesScreen(store, state, onOpened = { onRoute(Route.Chat(it.id)) })
            Route.Archive -> ArchiveScreen(store, state, onOpen = { onRoute(Route.Chat(it)) })
            Route.Artifacts -> ArtifactsScreen(store, state, me, platform, onOpen = { onRoute(Route.Artefact(it)) })
            is Route.Artefact -> ArtefactFullView(store, state, platform, route.id, onBack = { onRoute(Route.Artifacts) }, onOpenNodes = { onRoute(Route.Nodes) }, onOpenConversation = { onRoute(Route.Chat(it)) })
            Route.Assistants -> AssistantsScreen(store, state, onOpen = { onRoute(Route.AssistantSettings(it.id)) }, onNew = { onRoute(Route.NewAssistant) })
            Route.NewAssistant -> AssistantSettingsScreen(store, state, null, onDone = { onRoute(Route.Assistants) })
            is Route.AssistantSettings -> AssistantSettingsScreen(store, state, route.id, onDone = { onRoute(Route.Assistants) })
            Route.Activity -> ActivityScreen(store, state, onOpenConversation = { onRoute(Route.Chat(it)) })
            Route.Settings -> CompositionLocalProvider(LocalOpenConversation provides { onRoute(Route.Chat(it)) }) { SettingsScreen(store, me, store.api.baseUrl, startOn = settingsStart, onSignOut = onSignOut) }
        }
    }
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
private fun Sidebar(store: Store, platform: Platform, state: AppState, route: Route, onRoute: (Route) -> Unit, groupFilter: GroupFilterChoice, modifier: Modifier = Modifier) {
    val colors = MyTheme.colors
    val projectFilter = rememberProjectFilter(platform, state)
    var settledOpen by remember { mutableStateOf(platform.loadPreference(SettledOpenPreference) == true.toString()) }
    val pinned = pinnedConversations(state.conversations.values, groupFilter.value, projectFilter.value)
    val sections = sidebarSections(state.conversations.values, groupFilter.value, projectFilter.value)
    val pinnedChatRows = pinnedChats(state.conversations.values)
    val chats = chatSections(state.conversations.values)

    LaunchedEffect(Unit) {
        store.act { store.refreshGroups() }
        store.act { store.refreshProjects() }
    }

    LaunchedEffect(state.experiments.assistants) {
        store.act { store.refreshAssistants() }
    }
    var collapsed by remember { mutableStateOf(emptySet<String>()) }
    var usageOpen by remember { mutableStateOf(platform.loadPreference(UsageOpenPreference) == true.toString()) }
    var providerUsage by remember(store) { mutableStateOf(emptyList<ModelProviderUsage>()) }

    LaunchedEffect(store, usageOpen) {
        if (usageOpen) {
            while (true) {
                try {
                    providerUsage = store.api.modelProviderUsage()
                } catch (cancelled: CancellationException) {
                    throw cancelled
                } catch (_: Exception) {
                }

                delay(60_000L)
            }
        }
    }

    var mode by remember { mutableStateOf(if (route == Route.NewChat || (route as? Route.Chat)?.let { state.conversations[it.id]?.isChat } == true) ChatsMode else WorkspacesMode) }
    var lastOpened by remember { mutableStateOf(mapOf(WorkspacesMode to Route.New, ChatsMode to Route.NewChat)) }
    val openedMode = when (route) {
        Route.NewChat -> ChatsMode
        is Route.Chat -> state.conversations[route.id]?.let { if (it.isChat) ChatsMode else WorkspacesMode }
        Route.Settings, Route.Activity, Route.Empty -> null
        Route.Assistants, Route.NewAssistant, is Route.AssistantSettings -> null
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
            val choices = listOf(Choice(AllGroups, "All groups")) +
                state.groups.values.sortedBy { it.name.lowercase() }.map { group ->
                    val waiting = if (group.id == groupFilter.value) 0 else needsInputIn(state.conversations.values, group.id)

                    Choice(group.id, group.name, if (waiting > 0) "$waiting needs input" else null)
                } +
                Choice(NoGroup, "No group")

            Box(Modifier.padding(horizontal = Tokens.Space.S1_5)) {
                OptionMenu("Group", choices, groupFilter.value, groupFilter.choose, title = "Show", icon = MyIcon.Layers, size = ButtonSize.Sm)
            }
        }

        Hairline()

        Column(Modifier.fillMaxWidth().padding(Tokens.Space.S1_5)) {
            if (mode == ChatsMode) {
                SidebarLink("New chat", MyIcon.Plus, selected = route == Route.NewChat) { onRoute(Route.NewChat) }
            } else {
                SidebarLink("New conversation", MyIcon.Plus, selected = route == Route.New) { onRoute(Route.New) }
                SidebarLink("History", MyIcon.Clock, selected = route == Route.Archive) { onRoute(Route.Archive) }
                SidebarLink("Artefacts", MyIcon.Layers, selected = route == Route.Artifacts || route is Route.Artefact) { onRoute(Route.Artifacts) }

                if (state.experiments.tasks) {
                    SidebarLink("Tasks", MyIcon.List, selected = route == Route.Tasks) { onRoute(Route.Tasks) }
                }

                if (state.experiments.workflows) {
                    SidebarLink("Workflows", MyIcon.Layers, selected = route == Route.Workflows) { onRoute(Route.Workflows) }
                }

                SidebarLink("Schedules", MyIcon.Calendar, selected = route == Route.Schedules) { onRoute(Route.Schedules) }
            }
        }

        Hairline()

        val list = rememberLazyListState()

        LazyColumn(Modifier.weight(1f).fillMaxWidth().scrollbar(list), list, contentPadding = PaddingValues(horizontal = Tokens.Space.S1_5, vertical = Tokens.Space.S2)) {
            if (state.experiments.assistants) {
                sidebarAssistantItems(sidebarAssistants(state.assistants.values, groupFilter.value), route, onRoute)
            }

            if (mode == ChatsMode) {
                conversationRows(PinnedName, pinnedChatRows, state, store, route, onRoute, collapsed, { collapsed = it }, withNode = false)

                for (section in chats) {
                    conversationRows(section.name, section.rows, state, store, route, onRoute, collapsed, { collapsed = it }, withNode = false)
                }
            } else {
                item(key = "label") {
                    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                        Label("Conversations", Modifier.weight(1f).padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1), Tokens.Type.Meta, colors.textMuted)

                        if (state.projects.isNotEmpty()) {
                            val choices = listOf(Choice(AllProjects, "All projects")) + state.projects.values.sortedBy { it.name.lowercase() }.map { Choice(it.id, it.name) } + Choice(NoProject, "No project")

                            OptionMenu("Project", choices, projectFilter.value, projectFilter.choose, title = "Show", icon = MyIcon.Folder, size = ButtonSize.Sm)
                        }
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

            UsageCard(chatAccountFirst(agentAccounts(state.nodes.values), current?.agent, current?.let { state.nodes[it.nodeId]?.name }, current?.account), Modifier.padding(Tokens.Space.S1_5), providerUsage, current?.providerId, onUseReset = { account, key -> store.api.useReset(UseResetRequest(account.agent, account.label.takeUnless { it == UnnamedAccount }, key)).outcome })
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
                MyButton({ usageOpen = !usageOpen; platform.savePreference(UsageOpenPreference, usageOpen.toString()) }, icon = MyIcon.Gauge, description = "Usage", muted = true, selected = usageOpen)
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
            expanded = row.conversation.id !in collapsed,
            onToggle = { onCollapsed(if (row.conversation.id in collapsed) collapsed - row.conversation.id else collapsed + row.conversation.id) },
            onPin = { store.pin(row.conversation, row.conversation.pinnedAt == null) },
            onRename = { title -> store.rename(row.conversation, title) },
            onCloseSession = { store.closeSession(row.conversation) },
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
private fun SidebarLink(text: String, icon: MyIcon, selected: Boolean, onClick: () -> Unit) {
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
        Icon(icon, colors.textMuted, Tokens.Size.IconMd)
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
private fun ConversationRow(row: Conversation, node: NodeSummary?, project: Project?, selected: Boolean, onClick: () -> Unit, depth: Int = 0, sideChats: Int = 0, expanded: Boolean = true, onToggle: () -> Unit = {}, onPin: () -> Unit = {}, onRename: (String) -> Unit = {}, onCloseSession: () -> Unit = {}, onArchive: () -> Unit = {}, onCancelResume: () -> Unit = {}) {
    val colors = MyTheme.colors
    val platform = LocalPlatform.current
    var menuOpen by remember { mutableStateOf(false) }
    var renaming by remember { mutableStateOf(false) }
    val statusName = if (row.resumeAt != null) WaitingForReset else statusSectionName(row.status)
    val notRunning = node != null && (!node.online || row.id !in node.sessions)
    val indent = Tokens.Space.S3 * depth
    val projectName = project?.takeIf { !row.isChat }?.name
    val shownTitle = projectName?.let { "$it ${row.title}" } ?: row.title
    val place = listOfNotNull(row.branch, row.pullRequestLabel).joinToString("  ")

    Box(
        Modifier
            .fillMaxWidth()
            .pointerInput(Unit) { detectTapGestures(onLongPress = { menuOpen = true }) }
            .pointerInput(Unit) {
                awaitPointerEventScope {
                    while (true) {
                        val event = awaitPointerEvent()

                        if (event.type == PointerEventType.Press && event.buttons.isSecondaryPressed) {
                            menuOpen = true
                        }
                    }
                }
            },
        contentAlignment = Alignment.CenterStart,
    ) {
        if (menuOpen) {
            MenuPopup(onDismiss = { menuOpen = false }) {
                MenuItem(if (row.pinnedAt == null) "Pin" else "Unpin", onClick = { menuOpen = false; onPin() }, leading = MyIcon.Pin)
                MenuItem("Rename", onClick = { menuOpen = false; renaming = true }, leading = MyIcon.Pencil)

                if (!row.isChat) {
                    MenuItem("Close session", onClick = { menuOpen = false; onCloseSession() }, leading = MyIcon.Stop)
                }

                MenuItem("Archive", onClick = { menuOpen = false; onArchive() }, leading = MyIcon.Archive)
            }
        }

        if (renaming) {
            var title by remember { mutableStateOf(row.title) }
            val save = {
                renaming = false
                onRename(title.trim())
            }

            Dialog(onDismiss = { renaming = false }) {
                Label("Rename conversation", Modifier.semantics { heading() }, Tokens.Type.Title)
                TextInput(title, { title = it }, label = "Conversation name", placeholder = "Leave empty to use the suggested title", onSubmit = save)

                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End)) {
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
                    selected = selected,
                    shape = BlockShape,
                    description = "$shownTitle, $statusName, ${agentName(row.agent)}${node?.let { " on ${it.name}" }.orEmpty()}${if (notRunning) ", not running" else ""}${row.sideKind?.let { if (row.isSubagent) ", subagent" else ", side chat" }.orEmpty()}${if (!row.isChat && place.isNotEmpty()) ", " + listOfNotNull(row.branch?.let { "branch $it" }, row.pullRequestLabel).joinToString(", ") else ""}",
                )
                .padding(start = Tokens.Space.S2 + (if (row.isChat && sideChats == 0) 0.dp else Tokens.Space.S3) + indent, end = Tokens.Space.S2, top = Tokens.Space.S1_5, bottom = Tokens.Space.S1_5),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            val badge = when (row.status) {
                ConversationStatus.Working -> colors.statusWorking
                ConversationStatus.Input, ConversationStatus.Review -> colors.statusAttention
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

                if (row.resumeAt != null) {
                    Label(WaitingForReset, style = Tokens.Type.Meta, color = colors.statusAttention, maxLines = 1)
                }

                if (!row.isChat && place.isNotEmpty()) {
                    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
                        if (isWorktree(row, project)) {
                            Icon(MyIcon.Branch, colors.textMuted, Tokens.Size.IconSm)
                        }

                        Label(place, style = Tokens.Type.Meta, color = colors.textMuted)
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
                    ConversationStatus.Input, ConversationStatus.Review -> colors.text
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

        if (row.resumeAt != null) {
            Box(Modifier.align(Alignment.CenterEnd).padding(end = Tokens.Space.S2 * 3 + Tokens.Size.IconSm * 2)) {
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

private const val WaitingForReset = "Waiting for reset"

private const val UsageOpenPreference = "usage-card-open"

private val FooterLabelsWidth = 360.dp

private const val GroupFilterPreference = "conversation-group-filter"

private const val ProjectFilterPreference = "conversation-project-filter"

private const val SettledOpenPreference = "settled-open"

val LocalGroupFilter = staticCompositionLocalOf { AllGroups }

private class GroupFilterChoice(val value: String, val choose: (String) -> Unit)

@Composable
private fun rememberProjectFilter(platform: Platform, state: AppState): GroupFilterChoice {
    var saved by remember { mutableStateOf(platform.loadPreference(ProjectFilterPreference) ?: AllProjects) }
    val known = saved == AllProjects || saved == NoProject || saved in state.projects

    return GroupFilterChoice(if (known) saved else AllProjects) { chosen -> saved = chosen; platform.savePreference(ProjectFilterPreference, chosen) }
}

@Composable
private fun rememberGroupFilter(platform: Platform, state: AppState): GroupFilterChoice {
    var saved by remember { mutableStateOf(platform.loadPreference(GroupFilterPreference) ?: AllGroups) }
    val known = saved == AllGroups || saved == NoGroup || saved in state.groups

    return GroupFilterChoice(if (known) saved else AllGroups) { chosen -> saved = chosen; platform.savePreference(GroupFilterPreference, chosen) }
}
