package mycelium.ui

import androidx.compose.animation.core.Animatable
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.systemGestures
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.unit.IntOffset
import kotlinx.coroutines.launch
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
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import mycelium.Platform
import mycelium.api.Conversation
import mycelium.api.Me
import mycelium.api.NodeSummary
import mycelium.state.AppState
import mycelium.state.SidebarSection
import mycelium.state.Store
import androidx.compose.foundation.layout.size
import androidx.compose.ui.semantics.heading
import mycelium.api.ConversationStatus
import mycelium.state.isChat
import mycelium.state.chatSections
import mycelium.state.projectName
import mycelium.state.relativeTime
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
import mycelium.state.isLaptop
import mycelium.state.sidebarRows
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

    data object Schedules : Route

    data object Projects : Route

    data object Issues : Route

    data object Archive : Route

    data object Activity : Route

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
    var route by remember { mutableStateOf(initialRoute) }
    val colors = MyTheme.colors
    var sidebarWidth by remember { mutableStateOf(platform.loadPreference(SidebarWidthPreference)?.toFloatOrNull()?.dp ?: Tokens.Size.Sidebar) }
    val density = LocalDensity.current

    fun resize(width: Dp) {
        sidebarWidth = width.coerceIn(Tokens.Size.SidebarMin, Tokens.Size.SidebarMax)
        platform.savePreference(SidebarWidthPreference, sidebarWidth.value.toString())
    }

    Row(Modifier.fillMaxSize().background(colors.bgContent)) {
        Sidebar(
            state = state,
            route = route,
            onRoute = { route = it },
            modifier = Modifier.width(sidebarWidth).fillMaxHeight(),
        )

        ResizeHandle("Resize the sidebar") { moved -> resize(sidebarWidth + moved) }

        Column(Modifier.weight(1f).fillMaxHeight()) {
            problem?.let { ErrorBanner(it, onDismiss = store::dismissProblem) }

            Box(Modifier.weight(1f).fillMaxWidth()) {
                RouteContent(store, state, platform, me, route, onRoute = { route = it }, onSignOut = onSignOut)
            }
        }
    }
}

@OptIn(ExperimentalComposeUiApi::class)
@Composable
fun PhoneShell(store: Store, platform: Platform, me: Me, onSignOut: () -> Unit, initialRoute: Route = Route.Empty) {
    val state by store.state.collectAsState()
            if (!state.connected) {
                ReconnectingBanner()
            }

    val problem by store.problem.collectAsState()
    var route by remember { mutableStateOf(initialRoute) }
    var drawerOpen by remember { mutableStateOf(false) }
    val progress = remember { Animatable(0f) }
    val scope = rememberCoroutineScope()
    val reducedMotion = LocalReducedMotion.current
    val drawerShown by remember { derivedStateOf { progress.value > 0f } }
    val density = LocalDensity.current
    val layoutDirection = LocalLayoutDirection.current
    val gestureInset = with(density) { WindowInsets.systemGestures.getLeft(density, layoutDirection).toDp() }

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
        val drag = Modifier.draggable(
            state = rememberDraggableState { delta -> scope.launch { progress.snapTo((progress.value + delta / drawerWidthPx).coerceIn(0f, 1f)) } },
            orientation = Orientation.Horizontal,
            onDragStopped = { velocity -> settle(velocity > Tokens.Size.DrawerFlingVelocity || (velocity > -Tokens.Size.DrawerFlingVelocity && progress.value > 0.5f)) },
        )

        CompositionLocalProvider(LocalOpenList provides { drawerOpen = true }) {
            Column(Modifier.fillMaxSize()) {
                problem?.let { ErrorBanner(it, onDismiss = store::dismissProblem) }

                Box(Modifier.weight(1f).fillMaxWidth()) {
                    RouteContent(store, state, platform, me, route, onRoute = { route = it }, onSignOut = onSignOut)
                }
            }
        }

        if (!drawerShown) {
            Box(Modifier.padding(start = gestureInset).width(Tokens.Size.DrawerEdge).fillMaxHeight().then(drag))
        }

        if (drawerShown) {
                if (!state.connected) {
                    ReconnectingBanner()
                }

            Box(
                Modifier
                    .fillMaxSize()
                    .graphicsLayer { alpha = progress.value }
                    .background(Color.Black.copy(alpha = 0.32f))
                    .then(drag)
                    .clickable(interactionSource = remember { MutableInteractionSource() }, indication = null, role = Role.Button, onClickLabel = "Close conversations") { drawerOpen = false },
            )

            Sidebar(
                state = state,
                route = route,
                onRoute = { route = it; drawerOpen = false },
                modifier = Modifier
                    .width(drawerWidth)
                    .fillMaxHeight()
                    .offset { IntOffset((-(1f - progress.value) * drawerWidthPx).roundToInt(), 0) }
                    .then(drag),
            )
        }
    }
}

@Composable
private fun RouteContent(store: Store, state: AppState, platform: Platform, me: Me, route: Route, onRoute: (Route) -> Unit, onSignOut: () -> Unit) {
    CompositionLocalProvider(LocalPlatform provides platform) {
        when (route) {
            Route.Empty -> EmptyMain(state, onNew = { onRoute(Route.New) })
            is Route.Chat -> ConversationScreen(store, state, route.id, onArchived = { onRoute(Route.Empty) }, onOpenNodes = { onRoute(Route.Nodes) }, onOpenConversation = { onRoute(Route.Chat(it)) })
            Route.New -> NewConversationScreen(store, state, onCreated = { onRoute(Route.Chat(it.id)) })
            Route.NewChat -> NewChatScreen(store, onCreated = { onRoute(Route.Chat(it.id)) }, onOpenSettings = { onRoute(Route.Settings) })
            is Route.NewIn -> NewConversationScreen(store, state, onCreated = { onRoute(Route.Chat(it.id)) }, projectId = route.projectId)
            Route.Nodes -> NodesScreen(store, state, platform, onOpenNode = { onRoute(Route.Node(it)) })
            is Route.Node -> NodePage(store, state, route.id, platform, onBack = { onRoute(Route.Nodes) }, onOpenConversation = { onRoute(Route.Chat(it)) })
            Route.Tasks -> TasksScreen(store, state, platform, onOpen = { onRoute(Route.Chat(it)) })
            Route.Schedules -> SchedulesScreen(store, state)
            Route.Projects -> ProjectsScreen(store, state, onNewConversation = { onRoute(Route.NewIn(it)) })
            Route.Issues -> IssuesScreen(store, state, onOpened = { onRoute(Route.Chat(it.id)) })
            Route.Archive -> ArchiveScreen(store, state, onOpen = { onRoute(Route.Chat(it)) })
            Route.Activity -> ActivityScreen(store, state, onOpenConversation = { onRoute(Route.Chat(it)) })
            Route.Settings -> SettingsScreen(store, me, store.api.baseUrl, onSignOut)
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
private fun Sidebar(state: AppState, route: Route, onRoute: (Route) -> Unit, modifier: Modifier = Modifier) {
    val colors = MyTheme.colors
    val sections = sidebarSections(state.conversations.values)
    val chats = chatSections(state.conversations.values)
    var collapsed by remember { mutableStateOf(emptySet<String>()) }
    var mode by remember { mutableStateOf(if (route == Route.NewChat || (route as? Route.Chat)?.let { state.conversations[it.id]?.isChat } == true) ChatsMode else WorkspacesMode) }
    var lastOpened by remember { mutableStateOf(mapOf(WorkspacesMode to Route.New, ChatsMode to Route.NewChat)) }
    val openedMode = when (route) {
        Route.NewChat -> ChatsMode
        is Route.Chat -> state.conversations[route.id]?.let { if (it.isChat) ChatsMode else WorkspacesMode }
        Route.Settings, Route.Activity, Route.Empty -> null
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

        Hairline()

        Column(Modifier.fillMaxWidth().padding(Tokens.Space.S1_5)) {
            if (mode == ChatsMode) {
                SidebarLink("New chat", MyIcon.Plus, selected = route == Route.NewChat) { onRoute(Route.NewChat) }
            } else {
                SidebarLink("New conversation", MyIcon.Plus, selected = route == Route.New) { onRoute(Route.New) }
                SidebarLink("History", MyIcon.Clock, selected = route == Route.Archive) { onRoute(Route.Archive) }
                SidebarLink("Nodes", MyIcon.Server, selected = route == Route.Nodes || route is Route.Node) { onRoute(Route.Nodes) }
                SidebarLink("Tasks", MyIcon.List, selected = route == Route.Tasks) { onRoute(Route.Tasks) }
                SidebarLink("Schedules", MyIcon.Calendar, selected = route == Route.Schedules) { onRoute(Route.Schedules) }
            }
        }

        Hairline()

        val list = rememberLazyListState()

        LazyColumn(Modifier.weight(1f).fillMaxWidth().scrollbar(list), list, contentPadding = PaddingValues(horizontal = Tokens.Space.S1_5, vertical = Tokens.Space.S2)) {
            if (mode == ChatsMode) {
                for (section in chats) {
                    item(key = "chats-${section.name}") {
                        Label(section.name, Modifier.padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1).semantics { heading() }, Tokens.Type.Meta, colors.textMuted)
                    }

                    items(sidebarRows(section.rows, state.conversations.values, collapsed), key = { it.conversation.id }) { row ->
                        ConversationRow(
                            row.conversation,
                            null,
                            selected = row.conversation.id == (route as? Route.Chat)?.id,
                            onClick = { onRoute(Route.Chat(row.conversation.id)) },
                            depth = row.depth,
                            sideChats = row.sideChats,
                            expanded = row.conversation.id !in collapsed,
                            onToggle = { collapsed = if (row.conversation.id in collapsed) collapsed - row.conversation.id else collapsed + row.conversation.id },
                        )
                    }
                }
            } else {
                item(key = "label") {
                    Label("Conversations", Modifier.padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1), Tokens.Type.Meta, colors.textMuted)
                }

                for (section in sections) {
                    item(key = "section-${section.status}") {
                        SectionHeading(section)
                    }

                    items(sidebarRows(section.rows, state.conversations.values, collapsed), key = { it.conversation.id }) { row ->
                        ConversationRow(
                            row.conversation,
                            state.nodes[row.conversation.nodeId],
                            selected = row.conversation.id == (route as? Route.Chat)?.id,
                            onClick = { onRoute(Route.Chat(row.conversation.id)) },
                            depth = row.depth,
                            sideChats = row.sideChats,
                            expanded = row.conversation.id !in collapsed,
                            onToggle = { collapsed = if (row.conversation.id in collapsed) collapsed - row.conversation.id else collapsed + row.conversation.id },
                        )
                    }
                }
            }
        }

        Hairline()

        Row(
            Modifier.fillMaxWidth().heightIn(min = barHeight()).padding(horizontal = Tokens.Space.S1),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            val appearance = LocalAppearance.current

            if (mode == WorkspacesMode) {
                MyButton({ onRoute(Route.Projects) }, text = "Projects", icon = MyIcon.Folder, muted = true, selected = route == Route.Projects)
                MyButton({ onRoute(Route.Issues) }, text = "Issues", icon = MyIcon.Issue, muted = true, selected = route == Route.Issues)
            }

            Box(Modifier.weight(1f))
            MyButton(appearance.onToggleDark, icon = if (appearance.dark) MyIcon.Sun else MyIcon.Moon, description = if (appearance.dark) "Switch to light mode" else "Switch to dark mode", muted = true)
            MyButton({ onRoute(Route.Activity) }, icon = MyIcon.Activity, description = "Activity", muted = true, selected = route == Route.Activity)
            MyButton({ onRoute(Route.Settings) }, icon = MyIcon.Settings, description = "Settings", muted = true, selected = route == Route.Settings)
        }
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
private fun SectionHeading(section: SidebarSection) {
    Row(
        Modifier
            .fillMaxWidth()
            .padding(top = Tokens.Space.S2)
            .heightIn(min = rowHeight())
            .padding(horizontal = Tokens.Space.S2),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        StatusGlyph(section.status, Tokens.Size.IconMd)
        Label(section.name, Modifier.weight(1f).semantics { heading() }, Tokens.Type.UiStrong, MyTheme.colors.textMuted, maxLines = 1)
    }
}

@Composable
private fun ConversationRow(row: Conversation, node: NodeSummary?, selected: Boolean, onClick: () -> Unit, depth: Int = 0, sideChats: Int = 0, expanded: Boolean = true, onToggle: () -> Unit = {}) {
    val colors = MyTheme.colors
    val statusName = statusSectionName(row.status)
    val notRunning = node != null && (!node.online || row.id !in node.sessions)
    val indent = Tokens.Space.S3 * depth

    Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.CenterStart) {
        Row(
            Modifier
                .fillMaxWidth()
                .pressable(
                    onClick,
                    selected = selected,
                    shape = BlockShape,
                    description = "${row.title}, $statusName, ${agentName(row.agent)}${node?.let { " on ${it.name}" }.orEmpty()}${if (notRunning) ", not running" else ""}${row.sideKind?.let { ", side chat" }.orEmpty()}",
                )
                .padding(start = Tokens.Space.S2 + Tokens.Space.S3 + indent, end = Tokens.Space.S2, top = Tokens.Space.S1_5, bottom = Tokens.Space.S1_5),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(agentIcon(row.agent), colors.text, Tokens.Size.IconMd)
            Label(row.title, Modifier.weight(1f), color = if (notRunning) colors.textMuted else colors.text, maxLines = 1)

            Label(relativeTime(row.updatedAt), style = Tokens.Type.Meta, color = colors.textFaint, maxLines = 1)

            if (node != null) {
                Icon(nodeIcon(node), nodeColour(node), Tokens.Size.IconSm)
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
