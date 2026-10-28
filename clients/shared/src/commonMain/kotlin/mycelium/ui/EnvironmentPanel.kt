package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import kotlin.math.roundToInt
import kotlin.time.Clock
import kotlinx.coroutines.delay
import mycelium.Platform
import mycelium.api.BrowserTaskState
import mycelium.api.Conversation
import mycelium.api.Environment
import mycelium.api.EnvironmentAction
import mycelium.api.EnvironmentAppKind
import mycelium.api.EnvironmentController
import mycelium.api.EnvironmentSize
import mycelium.api.EnvironmentTarget
import mycelium.api.NodeSummary
import mycelium.state.AppState
import mycelium.state.EnvironmentStage
import mycelium.state.Store
import mycelium.state.catching
import mycelium.state.environmentActionAge
import mycelium.state.environmentAppsToStart
import mycelium.state.environmentCurrentWindow
import mycelium.state.environmentDefaultApps
import mycelium.state.environmentDescription
import mycelium.state.environmentMissingText
import mycelium.state.environmentStage
import mycelium.state.environmentKindText
import mycelium.state.environmentStartHint
import mycelium.state.environmentActivity
import mycelium.state.environmentStoppedText
import mycelium.state.environmentSizeText
import mycelium.state.environmentTools
import mycelium.state.environmentViewUrl
import mycelium.state.environmentWindowApp
import mycelium.state.nodeThatCanRunEnvironment

private const val ActionsShownInPanel = 6
private const val ActionsShownExpanded = 3
private const val ClockTickMillis = 1000L
private const val LiveViewUnavailable = "The live view is available in the desktop app."
private const val ControlNote = "The agent is paused and knows you took over. Your keyboard and pointer go to the environment while it has focus. Hand back when you are done and the agent carries on from what is on screen."
private val WindowsMenuWidth = 320.dp
private val TargetRadius = 14.dp
private val TargetChipHeight = 24.dp

@Composable
fun EnvironmentPanel(
    store: Store,
    state: AppState,
    conversation: Conversation,
    expanded: Boolean,
    onExpand: (Boolean) -> Unit,
    onClose: () -> Unit,
    onOpenNode: (String) -> Unit,
    onOpenConversation: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    val environment = state.environments[conversation.id]
    val node = state.nodes[conversation.nodeId]
    val stage = environmentStage(environment, node?.environment)
    val label = if (expanded) "Environment, expanded" else "Environment"

    Column(
        modifier
            .fillMaxSize()
            .background(MyTheme.colors.bgPanel)
            .semantics { contentDescription = label }
            .onKeyEvent { event ->
                if (expanded && event.type == KeyEventType.KeyDown && event.key == Key.Escape) {
                    onExpand(false)
                    true
                } else {
                    false
                }
            },
    ) {
        if (stage == EnvironmentStage.Active && environment != null) {
            ActiveEnvironment(store, state, conversation, environment, expanded, onExpand, onClose, onOpenNode)
        } else {
            ClosedEnvironment(store, state, conversation, stage, environment, node, onClose, onOpenNode, onOpenConversation)
        }
    }
}

@Composable
private fun ColumnScope.ClosedEnvironment(
    store: Store,
    state: AppState,
    conversation: Conversation,
    stage: EnvironmentStage,
    environment: Environment?,
    node: NodeSummary?,
    onClose: () -> Unit,
    onOpenNode: (String) -> Unit,
    onOpenConversation: (String) -> Unit,
) {
    val colors = MyTheme.colors
    val platform = LocalPlatform.current
    val nodeName = node?.name ?: environment?.nodeName ?: "this node"
    val capability = node?.environment
    val peer = nodeThatCanRunEnvironment(state.nodes.values, conversation.nodeId)
    var moving by remember(conversation.id) { mutableStateOf(false) }

    Bar {
        BarTitle("Environment")
        Spacer(Modifier.weight(1f))
        MyButton(onClose, icon = MyIcon.Close, description = "Close the environment panel", size = ButtonSize.Sm, muted = true)
    }

    Column(Modifier.weight(1f).fillMaxWidth().verticalScrollWithBar().padding(Tokens.Space.S4), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        val title = when (stage) {
            EnvironmentStage.Empty -> "No environment yet"
            EnvironmentStage.Starting -> "Starting the environment on $nodeName"
            EnvironmentStage.Failed -> "The environment did not start"
            EnvironmentStage.Stopped -> "Environment stopped"
            else -> "$nodeName cannot run an environment"
        }

        Label(title, Modifier.semantics { heading() }, Tokens.Type.Title)

        when (stage) {
            EnvironmentStage.Empty -> {
                val offered = capability?.apps.orEmpty()
                var ticked by remember(conversation.id, offered) { mutableStateOf(environmentDefaultApps(offered)) }

                Label("Start a separate desktop on $nodeName. The agent can look at it, click, type and scroll while you watch, and you can take over at any time. It never shows your own screen.", color = colors.textMuted)

                if (offered.isNotEmpty()) {
                    Label("Start with", Modifier.padding(top = Tokens.Space.S2).semantics { heading() }, Tokens.Type.UiStrong)
                    Label(environmentStartHint(nodeName), style = Tokens.Type.Meta, color = colors.textMuted)

                    for (app in offered) {
                        val isTicked = app.id in ticked

                        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                            Checkbox(isTicked, app.name, { ticked = if (isTicked) ticked - app.id else ticked + app.id }, checkedState = "Ticked", uncheckedState = "Not ticked")
                            Icon(environmentAppIcon(app.kind), colors.textMuted, Tokens.Size.IconMd, Modifier.clearAndSetSemantics {})
                            Label(app.name)
                        }
                    }
                }

                MyButton({ store.startEnvironment(conversation.id, if (offered.isEmpty()) null else environmentAppsToStart(offered, ticked)) }, text = "Start environment", variant = ButtonVariant.Primary)
            }

            EnvironmentStage.Starting -> {
                Label("Opening a Wayland session and your apps. This usually takes a few seconds.", color = colors.textMuted)
                MyButton({ store.stopEnvironment(conversation.id) }, text = "Cancel", variant = ButtonVariant.Outline)
            }

            EnvironmentStage.Failed -> {
                Label(environment?.failure ?: "$nodeName did not say why.", color = colors.textMuted)
                MyButton({ store.startEnvironment(conversation.id) }, text = "Try again", variant = ButtonVariant.Primary)
            }

            EnvironmentStage.Stopped -> {
                val stopped = environment?.let { environmentStoppedText(it) { millis -> platform?.localDateTime(millis) } }.orEmpty()

                val again = if (environment?.closedWithConversation == true) "Starting again reopens the conversation first." else "Starting again opens a fresh one."

                Label("$stopped $again", color = colors.textMuted)
                MyButton({ store.startEnvironment(conversation.id) }, text = "Start environment", variant = ButtonVariant.Primary)
            }

            else -> {
                val missing = capability?.missing.orEmpty()

                if (missing.isNotEmpty()) {
                    Label("${environmentMissingText(missing, nodeName)}. To use an environment here:", color = colors.textMuted)
                    Label("1.  Install ${environmentTools(missing)} on $nodeName with its package manager.", color = colors.textMuted)
                    Label("2.  Choose Check again.", color = colors.textMuted)
                } else {
                    Label(capability?.note ?: "$nodeName is missing something an environment needs.", color = colors.textMuted)
                }

                if (peer != null) {
                    Label("Or move this conversation to ${peer.name}, which can run one.", color = colors.textMuted)
                }

                Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                    if (peer != null) {
                        MyButton({ moving = true }, text = "Move to ${peer.name}", variant = ButtonVariant.Primary)
                    }

                    if (missing.isNotEmpty()) {
                        MyButton({ store.checkEnvironment(conversation.nodeId) }, text = "Check again", variant = ButtonVariant.Outline)
                    }
                }

                MyButton({ onOpenNode(conversation.nodeId) }, text = "$nodeName node page", size = ButtonSize.Sm, muted = true)
            }
        }
    }

    if (moving && peer != null) {
        MoveSheet(store, state, conversation, { moving = false }, onOpenConversation, peer.id)
    }
}

@Composable
private fun ColumnScope.ActiveEnvironment(
    store: Store,
    state: AppState,
    conversation: Conversation,
    environment: Environment,
    expanded: Boolean,
    onExpand: (Boolean) -> Unit,
    onClose: () -> Unit,
    onOpenNode: (String) -> Unit,
) {
    val colors = MyTheme.colors
    val platform = LocalPlatform.current
    val person = environment.controller == EnvironmentController.Person
    val phone = LocalTouch.current && expanded
    var now by remember { mutableStateOf(Clock.System.now()) }
    val actions = state.environmentActions[conversation.id].orEmpty()
    val apps = state.nodes[conversation.nodeId]?.environment?.apps.orEmpty()
    val currentWindow = environmentCurrentWindow(environment)
    var windowsOpen by remember { mutableStateOf(false) }

    LaunchedEffect(Unit) {
        while (true) {
            delay(ClockTickMillis)
            now = Clock.System.now()
        }
    }

    Bar {
        if (expanded && phone) {
            MyButton({ onExpand(false) }, icon = MyIcon.ChevronLeft, description = "Back to conversation", muted = true)
            Label("Environment", Modifier.semantics { heading() }, Tokens.Type.Title, maxLines = 1)
        } else if (expanded) {
            OpenListButton()
            MyButton({ onExpand(false) }, text = "Back to conversation", icon = MyIcon.ChevronLeft, muted = true)
            Label("Environment", Modifier.semantics { heading() }, Tokens.Type.Title, maxLines = 1)
        } else {
            BarTitle("Environment")
        }

        StatusMark(environmentActivity(environment), if (person) Tone.Attention else Tone.Muted, if (person) MyIcon.Person else MyIcon.Agent, Modifier.weight(1f))

        if (!phone && person) {
            MyButton({ store.handBackEnvironment(conversation.id) }, text = "Hand back", size = ButtonSize.Sm, variant = ButtonVariant.Primary)
        } else if (!phone) {
            MyButton({ store.takeOverEnvironment(conversation.id) }, text = "Take over", size = ButtonSize.Sm, variant = ButtonVariant.Outline)
        }

        if (!expanded) {
            MyButton({ onExpand(true) }, icon = MyIcon.Expand, description = "Expand the environment", size = ButtonSize.Sm, muted = true)
        }

        MyButton({ store.stopEnvironment(conversation.id) }, icon = MyIcon.Stop, description = "Stop the environment", size = ButtonSize.Sm, muted = true)

        if (!expanded) {
            MyButton(onClose, icon = MyIcon.Close, description = "Close the environment panel", size = ButtonSize.Sm, muted = true)
        }
    }

    val browserTask = environment.browserTask?.takeIf { it.state == BrowserTaskState.Running }

    if (browserTask != null) {
        Row(Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S1), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
            Icon(MyIcon.Globe, colors.textMuted, Tokens.Size.IconSm)
            Label(browserTask.goal, Modifier.weight(1f), Tokens.Type.Meta, colors.textMuted, maxLines = 1)
            MyButton({ store.stopBrowserTask(conversation.id) }, text = "Stop task", size = ButtonSize.Sm, variant = ButtonVariant.Outline)
        }
    }

    Row(Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S1), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
        MyButton({ onOpenNode(environment.nodeId) }, text = environment.nodeName, size = ButtonSize.Sm, muted = true, description = "${environment.nodeName} node page")
        Box {
            MyButton(
                { windowsOpen = true },
                text = currentWindow?.title ?: "Windows",
                icon = environmentAppIcon(apps.firstOrNull { it.id.equals(currentWindow?.appId, ignoreCase = true) }?.kind),
                size = ButtonSize.Sm,
                muted = true,
                description = "Windows, ${currentWindow?.title ?: "none open"}",
            )

            if (windowsOpen) {
                MenuPopup({ windowsOpen = false }, width = WindowsMenuWidth) {
                    MenuLabel("Windows")

                    if (environment.windows.isEmpty()) {
                        MenuLabel("No window is open yet")
                    }

                    for (window in environment.windows) {
                        MenuItem(
                            window.title,
                            {
                                windowsOpen = false
                                store.focusEnvironmentWindow(conversation.id, window.id)
                            },
                            checked = window.focused,
                            hint = environmentWindowApp(environment, window, apps),
                            leading = environmentAppIcon(apps.firstOrNull { it.id.equals(window.appId, ignoreCase = true) }?.kind),
                        )
                    }

                    if (apps.isNotEmpty()) {
                        MenuLabel("Open an app")
                    }

                    for (app in apps) {
                        MenuItem(
                            app.name,
                            {
                                windowsOpen = false
                                store.openEnvironmentApp(conversation.id, app.id)
                            },
                            hint = environmentKindText(app.kind),
                            leading = environmentAppIcon(app.kind),
                        )
                    }
                }
            }
        }

        if (currentWindow != null) {
            Label(environmentWindowApp(environment, currentWindow, apps), style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
        }

        Spacer(Modifier.weight(1f))
        Label(environmentSizeText(environment), style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
    }

    val ratio = environment.size.width.toFloat() / environment.size.height.coerceAtLeast(1)

    if (expanded) {
        Box(Modifier.weight(1f).fillMaxWidth().padding(Tokens.Space.S3), contentAlignment = Alignment.Center) {
            LiveStage(store, environment, platform, Modifier.fillMaxHeight().aspectRatio(ratio, matchHeightConstraintsFirst = true))
        }

        if (phone && person) {
            MyButton({ store.handBackEnvironment(conversation.id) }, text = "Hand back", modifier = Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S1), variant = ButtonVariant.Primary)
        } else if (phone) {
            MyButton({ store.takeOverEnvironment(conversation.id) }, text = "Take over", modifier = Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S1), variant = ButtonVariant.Outline)
        }

        Hairline()
        ActionsLog(actions, ActionsShownExpanded, now)

        return
    }

    Column(Modifier.weight(1f).fillMaxWidth().verticalScrollWithBar()) {
        Box(Modifier.fillMaxWidth().padding(Tokens.Space.S3)) {
            LiveStage(store, environment, platform, Modifier.fillMaxWidth().aspectRatio(ratio))
        }

        if (person) {
            Label(
                ControlNote,
                Modifier
                    .fillMaxWidth()
                    .padding(horizontal = Tokens.Space.S3)
                    .background(colors.bgContent, BlockShape)
                    .padding(Tokens.Space.S2)
                    .semantics { liveRegion = LiveRegionMode.Polite },
                Tokens.Type.Meta,
                colors.textMuted,
            )
        }

        Hairline(Modifier.padding(top = Tokens.Space.S2))
        ActionsLog(actions, ActionsShownInPanel, now)
    }
}

@Composable
private fun LiveStage(store: Store, environment: Environment, platform: Platform?, modifier: Modifier) {
    val colors = MyTheme.colors
    val person = environment.controller == EnvironmentController.Person
    var shown by remember { mutableStateOf(IntSize.Zero) }

    if (platform == null || !platform.showsLiveView) {
        Box(modifier.border(Tokens.Size.Hairline, colors.border, BlockShape).background(colors.bgContent, BlockShape), contentAlignment = Alignment.Center) {
            Label(LiveViewUnavailable, Modifier.padding(Tokens.Space.S3), color = colors.textMuted)
        }

        return
    }

    val connection = remember(platform) { platform.loadConnection() }
    var url by remember(environment.conversationId, environment.startedAt) { mutableStateOf<String?>(null) }

    LaunchedEffect(environment.conversationId, environment.startedAt) {
        val baseUrl = connection?.baseUrl ?: return@LaunchedEffect
        val ticket = catching { store.api.environmentStreamTicket(environment.conversationId) }.getOrNull() ?: return@LaunchedEffect

        url = environmentViewUrl(baseUrl, environment.conversationId, ticket)
    }
    val held = if (person) Modifier.border(Tokens.Size.FocusRing, colors.focus, BlockShape) else Modifier

    Box(
        modifier
            .semantics {
                role = Role.Image
                contentDescription = environmentDescription(environment)
            }
            .onSizeChanged { shown = it }
            .clip(BlockShape)
            .background(colors.bgContent, BlockShape)
            .then(held),
    ) {
        val viewUrl = url

        if (viewUrl != null) {
            platform.LiveView(viewUrl, !person, Modifier.fillMaxSize()) {
                if (!person) {
                    store.takeOverEnvironment(environment.conversationId)
                }
            }
        }

        val target = environment.target

        if (!person && target != null) {
            TargetMarker(target, environment.size, shown)
        }
    }
}

@Composable
private fun TargetMarker(target: EnvironmentTarget, session: EnvironmentSize, shown: IntSize) {
    val colors = MyTheme.colors
    val density = LocalDensity.current

    if (shown.width == 0 || session.width == 0 || session.height == 0) {
        return
    }

    val x = (target.x.toFloat() / session.width * shown.width).roundToInt()
    val y = (target.y.toFloat() / session.height * shown.height).roundToInt()
    val radius = with(density) { TargetRadius.roundToPx() }
    val chipHeight = with(density) { TargetChipHeight.roundToPx() }

    Box(
        Modifier
            .offset { IntOffset(x - radius, y - radius) }
            .size(TargetRadius * 2)
            .border(Tokens.Size.FocusRing, colors.accent, CircleShape)
            .clearAndSetSemantics {},
    )

    Icon(MyIcon.Pointer, colors.accent, Tokens.Size.IconMd, Modifier.offset { IntOffset(x, y) }.clearAndSetSemantics {})

    Label(
        target.label,
        Modifier
            .offset { IntOffset((x - radius).coerceAtLeast(0), (y - radius - chipHeight).coerceAtLeast(0)) }
            .background(colors.accent, ControlShape)
            .padding(horizontal = Tokens.Space.S1_5, vertical = Tokens.Space.S0_5)
            .clearAndSetSemantics {},
        Tokens.Type.Meta,
        colors.onAccent,
        maxLines = 1,
    )
}

@Composable
private fun ActionsLog(actions: List<EnvironmentAction>, limit: Int, now: kotlin.time.Instant) {
    val colors = MyTheme.colors

    Label("Actions", Modifier.padding(start = Tokens.Space.S3, end = Tokens.Space.S3, top = Tokens.Space.S2).semantics { heading() }, Tokens.Type.Meta, colors.textMuted)

    Column(Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S1).semantics { contentDescription = "Actions, newest first" }) {
        if (actions.isEmpty()) {
            Label("Nothing has happened in this environment yet.", style = Tokens.Type.Meta, color = colors.textMuted)
        }

        for (action in actions.take(limit)) {
            Row(Modifier.fillMaxWidth().padding(vertical = Tokens.Space.S0_5), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                Label(action.label, Modifier.weight(1f), Tokens.Type.Meta, maxLines = 1)
                Label(environmentActionAge(action.at, now), style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
            }
        }
    }
}

internal fun environmentAppIcon(kind: EnvironmentAppKind?): MyIcon = when (kind) {
    EnvironmentAppKind.Browser -> MyIcon.Globe
    EnvironmentAppKind.FileManager -> MyIcon.Folder
    EnvironmentAppKind.Editor -> MyIcon.Code
    EnvironmentAppKind.Office -> MyIcon.FileText
    EnvironmentAppKind.Terminal -> MyIcon.Terminal
    EnvironmentAppKind.Mail, EnvironmentAppKind.Other, null -> MyIcon.AppWindow
}
