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
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
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
import mycelium.state.agentName
import mycelium.state.isLaptop
import mycelium.state.sidebarSections
import mycelium.state.statusSectionName

sealed interface Route {
    data object Empty : Route

    data class Chat(val id: String) : Route

    data object New : Route

    data object Nodes : Route

    data object Archive : Route
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

    Row(Modifier.fillMaxSize().background(colors.bgContent)) {
        Sidebar(
            state = state,
            route = route,
            me = me,
            onRoute = { route = it },
            onSignOut = onSignOut,
            modifier = Modifier.width(Tokens.Size.Sidebar).fillMaxHeight(),
        )

        VerticalHairline()

        Column(Modifier.weight(1f).fillMaxHeight()) {
            problem?.let { ErrorBanner(it, onDismiss = store::dismissProblem) }

            Box(Modifier.weight(1f).fillMaxWidth()) {
                RouteContent(store, state, platform, route, onRoute = { route = it })
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
                    RouteContent(store, state, platform, route, onRoute = { route = it })
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
                me = me,
                onRoute = { route = it; drawerOpen = false },
                onSignOut = onSignOut,
                modifier = Modifier.width(minOf(Tokens.Size.Sidebar + Tokens.Space.S8 * 2, maxWidth * 0.86f)).fillMaxHeight(),
            )
        }
    }
}

@Composable
private fun RouteContent(store: Store, state: AppState, platform: Platform, route: Route, onRoute: (Route) -> Unit) {
    when (route) {
        Route.Empty -> EmptyMain(state, onNew = { onRoute(Route.New) })
        is Route.Chat -> ConversationScreen(store, state, route.id, onArchived = { onRoute(Route.Empty) }, onOpenNodes = { onRoute(Route.Nodes) })
        Route.New -> NewConversationScreen(store, state, onCreated = { onRoute(Route.Chat(it.id)) })
        Route.Nodes -> NodesScreen(store, state, platform)
        Route.Archive -> ArchiveScreen(store, state, onOpen = { onRoute(Route.Chat(it)) })
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
private fun Sidebar(state: AppState, route: Route, me: Me, onRoute: (Route) -> Unit, onSignOut: () -> Unit, modifier: Modifier = Modifier) {
    val colors = MyTheme.colors
    val collapsed = remember { mutableStateMapOf<String, Boolean>() }
    val sections = sidebarSections(state.conversations.values)

    Column(modifier.background(colors.bgPanel)) {
        Bar(chrome = true) {
            Label("Conversations", Modifier.weight(1f), Tokens.Type.UiStrong, maxLines = 1)
            MyButton({ onRoute(Route.New) }, icon = MyIcon.Plus, description = "New conversation", muted = true)
        }

        Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(top = Tokens.Space.S1, bottom = Tokens.Space.S2)) {
            for (section in sections) {
                SidebarSectionView(
                    section = section,
                    open = collapsed[section.name] != true,
                    onToggle = { collapsed[section.name] = collapsed[section.name] != true },
                    selectedId = (route as? Route.Chat)?.id,
                    nodes = state.nodes,
                    onOpen = { onRoute(Route.Chat(it)) },
                )
            }

            SidebarLink("All past conversations", MyIcon.Archive, selected = route == Route.Archive) { onRoute(Route.Archive) }
        }

        Hairline()

        Row(
            Modifier.fillMaxWidth().heightIn(min = barHeight()).padding(horizontal = Tokens.Space.S1),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            MyButton({ onRoute(Route.Nodes) }, icon = MyIcon.Server, description = "Nodes", muted = true, selected = route == Route.Nodes)
            Label(me.name.ifBlank { me.email }, style = Tokens.Type.Meta.copy(textAlign = TextAlign.End), color = colors.textMuted, maxLines = 1, modifier = Modifier.weight(1f))
            MyButton(onSignOut, icon = MyIcon.Logout, description = "Sign out ${me.email}", muted = true)
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
            .pressable(onClick, role = androidx.compose.ui.semantics.Role.Button, selected = selected)
            .padding(horizontal = Tokens.Space.S3),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(icon, colors.textMuted, Tokens.Size.IconSm)
        Label(text, color = colors.textMuted, maxLines = 1)
    }
}

@Composable
private fun SidebarSectionView(
    section: SidebarSection,
    open: Boolean,
    onToggle: () -> Unit,
    selectedId: String?,
    nodes: Map<String, NodeSummary>,
    onOpen: (String) -> Unit,
) {
    val colors = MyTheme.colors

    Column(Modifier.fillMaxWidth()) {
        Row(
            Modifier
                .fillMaxWidth()
                .heightIn(min = rowHeight())
                .pressable(onToggle, stateDescription = if (open) "Expanded" else "Collapsed")
                .padding(horizontal = Tokens.Space.S3),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(if (open) MyIcon.ChevronDown else MyIcon.ChevronRight, colors.textMuted, Tokens.Size.IconSm)
            Label(section.name, style = Tokens.Type.MetaStrong, color = colors.textMuted)
        }

        if (open) {
            for (row in section.rows) {
                ConversationRow(row, nodes[row.nodeId], selected = row.id == selectedId, onClick = { onOpen(row.id) })
            }
        }
    }
}

@Composable
private fun ConversationRow(row: Conversation, node: NodeSummary?, selected: Boolean, onClick: () -> Unit) {
    val colors = MyTheme.colors
    val statusName = statusSectionName(row.status)

    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = rowHeight())
            .pressable(
                onClick,
                selected = selected,
                description = "${row.title}, $statusName, ${agentName(row.agent)}${node?.let { " on ${it.name}" }.orEmpty()}",
            )
            .padding(start = Tokens.Space.S3, end = Tokens.Space.S2),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        StatusGlyph(row.status)
        Label(row.title, Modifier.weight(1f), maxLines = 1)
        Label(agentName(row.agent).substringBefore(' '), style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)

        if (node != null) {
            Icon(if (isLaptop(node.os)) MyIcon.Laptop else MyIcon.Server, colors.textMuted, Tokens.Size.IconSm)
        }
    }
}
