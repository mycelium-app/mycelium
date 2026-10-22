package mycelium.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
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
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
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
import mycelium.state.sidebarSections
import mycelium.state.statusSectionName

sealed interface Route {
    data object Empty : Route

    data class Chat(val id: String) : Route

    data object New : Route

    data class NewIn(val projectId: String) : Route

    data object Nodes : Route

    data object Projects : Route

    data object Issues : Route

    data object Archive : Route

    data object Settings : Route
}

@Composable
fun Shell(store: Store, platform: Platform, me: Me, onSignOut: () -> Unit, initialRoute: Route = Route.Empty) {
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

        val handle = remember { MutableInteractionSource() }
        val handleFocused by handle.collectIsFocusedAsState()

        Box(
            Modifier
                .fillMaxHeight()
                .width(Tokens.Size.Hairline)
                .background(if (handleFocused) colors.focus else colors.border)
                .semantics { contentDescription = "Resize the sidebar" },
        ) {
            Box(
                Modifier
                    .fillMaxHeight()
                    .requiredWidth(Tokens.Space.S2)
                    .pointerHoverIcon(HorizontalResizeIcon)
                    .draggable(rememberDraggableState { delta -> resize(sidebarWidth + with(density) { delta.toDp() }) }, Orientation.Horizontal)
                    .onKeyEvent { event ->
                        val step = when (event.key) {
                            Key.DirectionLeft -> -Tokens.Space.S4
                            Key.DirectionRight -> Tokens.Space.S4
                            else -> null
                        }

                        if (event.type == KeyEventType.KeyDown && step != null) {
                            resize(sidebarWidth + step)
                        }

                        step != null
                    }
                    .focusable(interactionSource = handle),
            )
        }

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
    val problem by store.problem.collectAsState()
    var route by remember { mutableStateOf(initialRoute) }
    var drawerOpen by remember { mutableStateOf(false) }

    BackHandler(enabled = drawerOpen) { drawerOpen = false }

    BoxWithConstraints(Modifier.fillMaxSize().background(MyTheme.colors.bgContent)) {
        CompositionLocalProvider(LocalOpenList provides { drawerOpen = true }) {
            Column(Modifier.fillMaxSize()) {
                problem?.let { ErrorBanner(it, onDismiss = store::dismissProblem) }

                Box(Modifier.weight(1f).fillMaxWidth()) {
                    RouteContent(store, state, platform, me, route, onRoute = { route = it }, onSignOut = onSignOut)
                }
            }
        }

        AnimatedVisibility(drawerOpen, enter = fadeIn(), exit = fadeOut()) {
            Box(
                Modifier
                    .fillMaxSize()
                    .background(Color.Black.copy(alpha = 0.32f))
                    .clickable(interactionSource = remember { MutableInteractionSource() }, indication = null, role = Role.Button, onClickLabel = "Close conversations") { drawerOpen = false },
            )
        }

        AnimatedVisibility(drawerOpen, enter = slideInHorizontally { -it }, exit = slideOutHorizontally { -it }) {
            Sidebar(
                state = state,
                route = route,
                onRoute = { route = it; drawerOpen = false },
                modifier = Modifier.width(minOf(Tokens.Size.Sidebar + Tokens.Space.S8 * 2, maxWidth * 0.86f)).fillMaxHeight(),
            )
        }
    }
}

@Composable
private fun RouteContent(store: Store, state: AppState, platform: Platform, me: Me, route: Route, onRoute: (Route) -> Unit, onSignOut: () -> Unit) {
    when (route) {
        Route.Empty -> EmptyMain(state, onNew = { onRoute(Route.New) })
        is Route.Chat -> ConversationScreen(store, state, route.id, onArchived = { onRoute(Route.Empty) }, onOpenNodes = { onRoute(Route.Nodes) })
        Route.New -> NewConversationScreen(store, state, onCreated = { onRoute(Route.Chat(it.id)) })
        is Route.NewIn -> NewConversationScreen(store, state, onCreated = { onRoute(Route.Chat(it.id)) }, projectId = route.projectId)
        Route.Nodes -> NodesScreen(store, state, platform)
        Route.Projects -> ProjectsScreen(store, state, onNewConversation = { onRoute(Route.NewIn(it)) })
        Route.Issues -> IssuesScreen(store, state, onOpened = { onRoute(Route.Chat(it.id)) })
        Route.Archive -> ArchiveScreen(store, state, onOpen = { onRoute(Route.Chat(it)) })
        Route.Settings -> SettingsScreen(store, me, store.api.baseUrl, onSignOut)
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

@Composable
private fun Sidebar(state: AppState, route: Route, onRoute: (Route) -> Unit, modifier: Modifier = Modifier) {
    val colors = MyTheme.colors
    val sections = sidebarSections(state.conversations.values)

    Column(modifier.background(colors.bgPanel)) {
        Column(Modifier.fillMaxWidth().padding(Tokens.Space.S1_5)) {
            SidebarLink("New conversation", MyIcon.Plus, selected = route == Route.New) { onRoute(Route.New) }
            SidebarLink("History", MyIcon.Clock, selected = route == Route.Archive) { onRoute(Route.Archive) }
            SidebarLink("Nodes", MyIcon.Server, selected = route == Route.Nodes) { onRoute(Route.Nodes) }
        }

        Hairline()

        LazyColumn(Modifier.weight(1f).fillMaxWidth(), contentPadding = PaddingValues(horizontal = Tokens.Space.S1_5, vertical = Tokens.Space.S2)) {
            item(key = "label") {
                Label("Conversations", Modifier.padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1), Tokens.Type.Meta, colors.textMuted)
            }

            for (section in sections) {
                item(key = "section-${section.status}") {
                    SectionHeading(section)
                }

                items(section.rows, key = { it.id }) { row ->
                    ConversationRow(row, state.nodes[row.nodeId], selected = row.id == (route as? Route.Chat)?.id, onClick = { onRoute(Route.Chat(row.id)) })
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

            MyButton({ onRoute(Route.Projects) }, text = "Projects", icon = MyIcon.Folder, muted = true, selected = route == Route.Projects)
            MyButton({ onRoute(Route.Issues) }, text = "Issues", icon = MyIcon.Issue, muted = true, selected = route == Route.Issues)
            Box(Modifier.weight(1f))
            MyButton(appearance.onToggleDark, icon = if (appearance.dark) MyIcon.Sun else MyIcon.Moon, description = if (appearance.dark) "Switch to light mode" else "Switch to dark mode", muted = true)
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
private fun ConversationRow(row: Conversation, node: NodeSummary?, selected: Boolean, onClick: () -> Unit) {
    val colors = MyTheme.colors
    val statusName = statusSectionName(row.status)

    Row(
        Modifier
            .fillMaxWidth()
            .pressable(
                onClick,
                selected = selected,
                shape = BlockShape,
                description = "${row.title}, $statusName, ${agentName(row.agent)}${node?.let { " on ${it.name}" }.orEmpty()}",
            )
            .padding(start = Tokens.Space.S2 + Tokens.Space.S3, end = Tokens.Space.S2, top = Tokens.Space.S1_5, bottom = Tokens.Space.S1_5),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(agentIcon(row.agent), colors.text, Tokens.Size.IconMd)
        Label(row.title, Modifier.weight(1f), maxLines = 1)

        if (node != null) {
            Icon(nodeIcon(node), nodeColour(node.name), Tokens.Size.IconSm)
        }

        Label(relativeTime(row.updatedAt), style = Tokens.Type.Meta, color = colors.textFaint, maxLines = 1)
    }
}

@Composable
fun nodeColour(name: String): Color {
    val choices = Accent.entries.filter { it != Accent.System && it != Accent.Neutral }
    val accent = choices[name.hashCode().mod(choices.size)]

    return if (LocalAppearance.current.dark) accent.dark else accent.light
}

private const val SidebarWidthPreference = "sidebar-width"
