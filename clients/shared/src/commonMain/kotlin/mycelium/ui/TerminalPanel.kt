package mycelium.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.focusable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.key
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.draw.drawBehind
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.runtime.SideEffect
import androidx.compose.foundation.gestures.Orientation
import androidx.compose.ui.unit.Dp
import mycelium.api.ConversationStatus
import mycelium.state.AgentTerminalActivity
import mycelium.state.TerminalWorkspace
import mycelium.state.agentTerminalActivity
import mycelium.state.dividers
import mycelium.state.paneIds
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEvent
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.isAltPressed
import androidx.compose.ui.input.key.isCtrlPressed
import androidx.compose.ui.input.key.isMetaPressed
import androidx.compose.ui.input.key.isShiftPressed
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.input.key.utf16CodePoint
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.IntSize
import kotlin.io.encoding.Base64
import kotlin.math.floor
import kotlinx.coroutines.delay
import mycelium.api.Conversation
import mycelium.state.AppState
import mycelium.state.CellStyle
import mycelium.state.Store
import mycelium.state.SplitDirection
import mycelium.state.Terminal
import mycelium.state.TerminalColor
import mycelium.state.TerminalKey
import mycelium.state.TerminalTabs
import mycelium.state.rects
import mycelium.state.codePointString
import mycelium.state.projectName
import mycelium.state.terminalControlInput
import mycelium.state.terminalCursorShown
import mycelium.state.terminalKeyInput
import mycelium.state.xtermRgb
import androidx.compose.ui.unit.dp

@Composable
fun TerminalPanel(store: Store, state: AppState, conversation: Conversation, modifier: Modifier = Modifier, onExited: () -> Unit = {}) {
    val colors = MyTheme.colors
    val id = conversation.id
    var workspace by remember(id) { mutableStateOf(store.terminalWorkspaces[id] ?: TerminalWorkspace()) }
    var scrollbacks by remember(id) { mutableStateOf<Map<String, String>>(emptyMap()) }
    var loaded by remember(id) { mutableStateOf(false) }
    val tabs = workspace.tabs
    val current = tabs.current
    val activity = agentTerminalActivity(state.events[id].orEmpty(), conversation.status == ConversationStatus.Working)

    fun change(next: (TerminalTabs) -> TerminalTabs) {
        val before = workspace.tabs.tabs.flatMap { it.layout.paneIds() }.toSet()
        val changed = next(workspace.tabs)
        val removed = before - changed.tabs.flatMap { it.layout.paneIds() }.toSet()

        removed.forEach { pane -> workspace.panes[pane]?.let(store::stopTerminal) }
        workspace = workspace.copy(tabs = changed, panes = workspace.panes - removed)
    }

    LaunchedEffect(id) {
        val open = runCatching { store.api.openTerminals(id) }.getOrDefault(emptyList())
        val running = open.map { it.terminalId }.toSet()
        val gone = workspace.panes.filterValues { it !in running }.keys
        var restored = workspace.copy(tabs = gone.fold(workspace.tabs) { result, pane -> result.closePane(pane) }, panes = workspace.panes - gone)

        if (restored.tabs.tabs.isEmpty()) {
            restored = if (open.isEmpty()) restored.copy(tabs = TerminalTabs().open()) else TerminalWorkspace().adopting(open.map { it.terminalId })
        }

        scrollbacks = open.associate { it.terminalId to it.scrollback }
        workspace = restored
        loaded = true
    }

    SideEffect {
        if (loaded) {
            store.terminalWorkspaces[id] = workspace
        }
    }

    LaunchedEffect(loaded, tabs.tabs.isEmpty()) {
        if (loaded && tabs.tabs.isEmpty()) {
            onExited()
        }
    }

    Column(modifier.background(colors.bgPanel).semantics { contentDescription = "Terminal" }) {
        Row(Modifier.fillMaxWidth().height(Tokens.Size.Bar).padding(horizontal = Tokens.Space.S2), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
            MainTabStrip(
                tabs = tabs.tabs.map { tab ->
                    val seen = tab.layout.paneIds().mapNotNull { pane -> workspace.panes[pane]?.let(activity::get) }
                    val agent = if (AgentTerminalActivity.Typing in seen) AgentTerminalActivity.Typing else seen.firstOrNull()

                    MainTab(
                        tab.id,
                        tab.title,
                        "Close ${tab.title}",
                        badge = when (agent) {
                            AgentTerminalActivity.Typing -> MyIcon.Keyboard
                            AgentTerminalActivity.Reading -> MyIcon.Eye
                            null -> null
                        },
                        badgeLabel = if (agent == AgentTerminalActivity.Typing) "Agent is typing in this terminal" else "Agent is reading this terminal",
                    )
                },
                selected = tabs.selected.orEmpty(),
                onSelect = { workspace = workspace.copy(tabs = tabs.select(it)) },
                onClose = { tabId -> change { it.closeTab(tabId) } },
                groupLabel = "Terminals",
                modifier = Modifier.weight(1f),
            )
            MyButton({ change { it.split(SplitDirection.Right) } }, icon = MyIcon.SplitRight, description = "Split right", muted = true)
            MyButton({ change { it.split(SplitDirection.Down) } }, icon = MyIcon.SplitDown, description = "Split down", muted = true)
            MyButton({ change { it.open() } }, icon = MyIcon.Plus, description = "New terminal", muted = true)
            MyButton({ change { it.closeFocused() } }, icon = MyIcon.Close, description = "Close terminal", muted = true)
        }

        Hairline()

        BoxWithConstraints(Modifier.weight(1f).fillMaxWidth()) {
            for (tab in tabs.tabs.takeIf { loaded }.orEmpty()) {
                key(tab.id) {
                    val shown = tab.id == current?.id

                    for ((paneId, rect) in tab.layout.rects()) {
                        key(paneId) {
                            val placement = if (shown) Modifier.offset(maxWidth * rect.x, maxHeight * rect.y).size(maxWidth * rect.width, maxHeight * rect.height) else Modifier.size(0.dp)
                            val existing = workspace.panes[paneId]

                            TerminalView(
                                store,
                                state,
                                conversation,
                                placement.clipToBounds().drawBehind {
                                    if (rect.x > 0f) {
                                        drawLine(colors.border, Offset(0f, 0f), Offset(0f, size.height))
                                    }

                                    if (rect.y > 0f) {
                                        drawLine(colors.border, Offset(0f, 0f), Offset(size.width, 0f))
                                    }
                                },
                                existing = existing,
                                scrollback = existing?.let(scrollbacks::get),
                                active = shown && tab.focused == paneId,
                                onStarted = { terminalId -> workspace = workspace.copy(panes = workspace.panes + (paneId to terminalId)) },
                                onFocus = { workspace = workspace.copy(tabs = workspace.tabs.focus(paneId)) },
                                onExited = { change { it.closePane(paneId) } },
                            )
                        }
                    }
                }
            }

            for (divider in current?.layout?.dividers().orEmpty()) {
                val area = divider.area
                val across = if (divider.direction == SplitDirection.Right) maxWidth * area.width else maxHeight * area.height
                val resize = { moved: Dp -> workspace = workspace.copy(tabs = workspace.tabs.resize(current!!.id, divider.path, divider.ratio + moved / across)) }

                if (divider.direction == SplitDirection.Right) {
                    Box(Modifier.offset(maxWidth * (area.x + area.width * divider.ratio), maxHeight * area.y).height(maxHeight * area.height).width(Tokens.Size.Hairline)) {
                        ResizeHandle("Resize the split", onMove = resize)
                    }
                } else {
                    Box(Modifier.offset(maxWidth * area.x, maxHeight * (area.y + area.height * divider.ratio)).width(maxWidth * area.width).height(Tokens.Size.Hairline)) {
                        ResizeHandle("Resize the split", Orientation.Vertical, resize)
                    }
                }
            }
        }
    }
}

@Composable
private fun TerminalView(store: Store, state: AppState, conversation: Conversation, modifier: Modifier, existing: String?, scrollback: String?, active: Boolean, onStarted: (String) -> Unit, onFocus: () -> Unit, onExited: () -> Unit) {
    val colors = MyTheme.colors
    val touch = LocalTouch.current
    val density = LocalDensity.current
    val measurer = rememberTextMeasurer()
    val style = Tokens.Type.Code.themed()
    val measuredWidth = remember(style, measurer) { measurer.measure("M".repeat(CellProbeLength), style).size.width.toFloat() / CellProbeLength }
    val cellWidth = if (measuredWidth > 0f) measuredWidth else with(density) { style.fontSize.toPx() * FallbackCellRatio }
    val reducedMotion = LocalReducedMotion.current
    var blinkOn by remember { mutableStateOf(true) }
    val cellHeight = with(density) { style.lineHeight.toPx() }
    val ansi = ansiColours(colors)
    val source = remember { MutableInteractionSource() }
    val focused = source.collectIsFocusedAsState()
    val focusRequester = remember { FocusRequester() }
    var attempt by remember(conversation.id) { mutableIntStateOf(0) }
    var area by remember { mutableStateOf(IntSize.Zero) }
    var terminalId by remember(conversation.id, attempt) { mutableStateOf(existing.takeIf { attempt == 0 }) }
    var failure by remember(conversation.id, attempt) { mutableStateOf<String?>(null) }
    val columns = if (cellWidth > 0f) (area.width / cellWidth).toInt() else 0
    val rows = if (cellHeight > 0f) (area.height / cellHeight).toInt() else 0
    val emulator = remember(conversation.id, attempt) { Terminal(DefaultColumns, DefaultRows) }
    var revision by remember(emulator) { mutableIntStateOf(0) }
    val sentSize = remember(emulator) { intArrayOf(0, 0) }
    val consumed = remember(emulator) { intArrayOf(existing?.let { state.terminalOutput[it]?.received } ?: 0) }
    val id = terminalId
    val exited = id != null && id in state.exitedTerminals
    val feed = id?.let { state.terminalOutput[it] }

    LaunchedEffect(exited) {
        if (exited) {
            onExited()
        }
    }

    LaunchedEffect(emulator, columns > 0 && rows > 0) {
        if (columns > 0 && rows > 0 && terminalId == null) {
            failure = attempt {
                val started = store.api.startTerminal(conversation.id, columns, rows).terminalId
                terminalId = started
                onStarted(started)
                sentSize[0] = columns
                sentSize[1] = rows
            }
        }
    }

    LaunchedEffect(id, columns, rows) {
        if (columns > 0 && rows > 0) {
            emulator.resize(columns, rows)
            revision++

            if (id != null && !exited && (sentSize[0] != columns || sentSize[1] != rows)) {
                sentSize[0] = columns
                sentSize[1] = rows
                store.act { store.api.resizeTerminal(id, columns, rows) }
            }
        }
    }

    LaunchedEffect(emulator) {
        if (existing != null && scrollback != null) {
            emulator.feed(Base64.decode(scrollback))
            revision++
        }
    }

    LaunchedEffect(id, feed?.received) {
        if (feed != null) {
            feed.unseen(consumed[0]).forEach { emulator.feed(Base64.decode(it)) }
            consumed[0] = feed.received
            revision++
        }
    }

    LaunchedEffect(reducedMotion, revision) {
        blinkOn = true

        while (!reducedMotion) {
            delay(CursorBlinkMillis)
            blinkOn = !blinkOn
        }
    }

    LaunchedEffect(id, active) {
        if (id != null && active && !touch) {
            focusRequester.requestFocus()
        }
    }

    var selecting by remember(emulator) { mutableStateOf(false) }

    Column(modifier) {
        if (LocalAnnotate.current != null && id != null) {
            Row(Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S2), horizontalArrangement = Arrangement.End) {
                MyButton({ selecting = !selecting }, text = if (selecting) "Back to terminal" else "Select output", size = ButtonSize.Sm, muted = true)
            }
        }

        if (selecting) {
            Box(Modifier.weight(1f).fillMaxWidth().background(colors.bgContent).verticalScrollWithBar().padding(Tokens.Space.S2).semantics { contentDescription = "Terminal output" }) {
                Annotatable {
                    Label(emulator.screenText().trimEnd(), style = Tokens.Type.Code)
                }
            }

            return@Column
        }

        Box(
            Modifier
                .weight(1f)
                .fillMaxWidth()
                .background(colors.bgContent)
                .padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1)
                .onSizeChanged { area = it }
                .focusRing(focused, inside = true)
                .focusRequester(focusRequester)
                .onPreviewKeyEvent { event ->
                    val typed = if (id == null || exited) null else terminalInputFor(event, emulator.applicationCursorKeys)

                    if (typed != null) {
                        store.typeIntoTerminal(id!!, typed)
                    }

                    typed != null
                }
                .focusable(interactionSource = source)
                .pointerInput(Unit) { detectTapGestures { onFocus(); focusRequester.requestFocus() } }
                .semantics { contentDescription = "Terminal for ${projectName(conversation.cwd)}. Press Shift and Tab to leave it." },
        ) {
            Canvas(Modifier.fillMaxSize()) {
                revision.let { drawEmulator(emulator, measurer, style, cellWidth, cellHeight, ansi, colors.text, colors.bgContent) }
            }

            Canvas(Modifier.fillMaxSize()) {
                revision.let { drawCursor(emulator, cellWidth, cellHeight, colors.text, focused.value, blinkOn) }
            }
        }

        if (failure != null) {
            Row(
                Modifier.fillMaxWidth().background(colors.bgChrome).padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S1),
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Label(failure.orEmpty(), Modifier.weight(1f), Tokens.Type.Meta, colors.textMuted)
                MyButton({ attempt++ }, text = "Open again", size = ButtonSize.Sm, variant = ButtonVariant.Outline)
            }
        }

        if (touch && id != null && !exited) {
            TerminalKeys(onType = { store.typeIntoTerminal(id, it) }, applicationCursorKeys = emulator.applicationCursorKeys)
        }
    }
}

@Composable
private fun TerminalKeys(onType: (String) -> Unit, applicationCursorKeys: Boolean) {
    var draft by remember { mutableStateOf("") }

    Column(Modifier.fillMaxWidth().background(MyTheme.colors.bgChrome).padding(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
        TextInput(
            draft,
            { draft = it },
            label = "Send to terminal",
            placeholder = "Type, then send with Enter",
            mono = true,
            showLabel = false,
            onSubmit = {
                onType(draft + terminalKeyInput(TerminalKey.Enter))
                draft = ""
            },
        )

        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            MyButton({ onType(terminalKeyInput(TerminalKey.Escape)) }, text = "Esc", variant = ButtonVariant.Outline)
            MyButton({ onType(terminalKeyInput(TerminalKey.Tab)) }, text = "Tab", variant = ButtonVariant.Outline)
            MyButton({ onType(terminalControlInput('c').orEmpty()) }, text = "Ctrl+C", variant = ButtonVariant.Outline)
            MyButton({ onType(terminalKeyInput(TerminalKey.Up, applicationCursorKeys)) }, text = "Up", variant = ButtonVariant.Outline)
            MyButton({ onType(terminalKeyInput(TerminalKey.Down, applicationCursorKeys)) }, text = "Down", variant = ButtonVariant.Outline)
        }
    }
}

private val LetterKeys = listOf(
    Key.A, Key.B, Key.C, Key.D, Key.E, Key.F, Key.G, Key.H, Key.I, Key.J, Key.K, Key.L, Key.M,
    Key.N, Key.O, Key.P, Key.Q, Key.R, Key.S, Key.T, Key.U, Key.V, Key.W, Key.X, Key.Y, Key.Z,
)

private const val CellProbeLength = 20
private const val DefaultColumns = 80
private const val DefaultRows = 24
private const val FallbackCellRatio = 0.6f
private const val CursorBlinkMillis = 530L
private const val CursorBlockAlpha = 0.6f

private val ModifierKeys = setOf(Key.ShiftLeft, Key.ShiftRight, Key.CtrlLeft, Key.CtrlRight, Key.AltLeft, Key.AltRight, Key.MetaLeft, Key.MetaRight, Key.CapsLock, Key.Function)
private val PrivateUseCodePoints = 0xE000..0xF8FF

private fun terminalInputFor(event: KeyEvent, applicationCursorKeys: Boolean): String? {
    if (event.type != KeyEventType.KeyDown) {
        return null
    }

    val special = when (event.key) {
        Key.Enter, Key.NumPadEnter -> TerminalKey.Enter
        Key.Backspace -> TerminalKey.Backspace
        Key.Tab -> if (event.isShiftPressed) return null else TerminalKey.Tab
        Key.Escape -> TerminalKey.Escape
        Key.DirectionUp -> TerminalKey.Up
        Key.DirectionDown -> TerminalKey.Down
        Key.DirectionRight -> TerminalKey.Right
        Key.DirectionLeft -> TerminalKey.Left
        Key.MoveHome -> TerminalKey.Home
        Key.MoveEnd -> TerminalKey.End
        Key.PageUp -> TerminalKey.PageUp
        Key.PageDown -> TerminalKey.PageDown
        Key.Delete -> TerminalKey.Delete
        else -> null
    }

    if (special != null) {
        return terminalKeyInput(special, applicationCursorKeys)
    }

    if (event.isMetaPressed) {
        return null
    }

    if (event.isCtrlPressed) {
        val index = LetterKeys.indexOf(event.key)

        return if (index < 0) null else terminalControlInput('a' + index)
    }

    val codePoint = event.utf16CodePoint

    if (event.key in ModifierKeys || codePoint < ' '.code || codePoint == 0x7F || codePoint in PrivateUseCodePoints || codePoint >= 0xFFFE) {
        return null
    }

    val text = codePointString(codePoint)

    return if (event.isAltPressed) "\u001B$text" else text
}

private fun ansiColours(colors: Palette): List<Color> {
    val dark = colors.bgContent.luminance() < 0.5f
    val brighter = { color: Color -> lerp(color, colors.text, 0.3f) }
    val chromatic = listOf(
        colors.statusFailed,
        colors.diffAdd,
        colors.statusAttention,
        colors.statusWorking,
        lerp(colors.statusFailed, colors.statusWorking, 0.5f),
        lerp(colors.diffAdd, colors.statusWorking, 0.5f),
    )

    return listOf(if (dark) colors.bgSelected else colors.text) +
        chromatic +
        listOf(if (dark) colors.textMuted else colors.bgSelected) +
        listOf(colors.textFaint) +
        chromatic.map(brighter) +
        listOf(if (dark) colors.text else colors.bgContent)
}

private fun resolve(color: TerminalColor, ansi: List<Color>, fallback: Color): Color = when (color) {
    TerminalColor.Default -> fallback
    is TerminalColor.Indexed -> if (color.index < ansi.size) {
        ansi[color.index]
    } else {
        val (red, green, blue) = xtermRgb(color.index)

        Color(red, green, blue)
    }
    is TerminalColor.Rgb -> Color(color.red, color.green, color.blue)
}

private fun androidx.compose.ui.graphics.drawscope.DrawScope.drawEmulator(
    emulator: Terminal,
    measurer: androidx.compose.ui.text.TextMeasurer,
    style: androidx.compose.ui.text.TextStyle,
    cellWidth: Float,
    cellHeight: Float,
    ansi: List<Color>,
    defaultForeground: Color,
    defaultBackground: Color,
) {
    for (row in 0 until emulator.rows) {
        val top = row * cellHeight
        val pixelTop = floor(top)
        val pixelHeight = floor((row + 1) * cellHeight) - pixelTop
        var fillColour = Color.Transparent
        var fillStart = 0
        val paint = { end: Int ->
            if (fillColour != Color.Transparent) {
                val left = floor(fillStart * cellWidth)
                drawRect(fillColour, Offset(left, pixelTop), Size(floor(end * cellWidth) - left, pixelHeight))
            }
        }
        val text = buildAnnotatedString {
            var runStyle: SpanStyle? = null
            val run = StringBuilder()
            val flush = {
                if (run.isNotEmpty()) {
                    withStyle(runStyle ?: SpanStyle()) { append(run.toString()) }
                    run.clear()
                }
            }

            for (column in 0 until emulator.columns) {
                val cell = emulator.cell(row, column)
                val inverse = cell.style.inverse
                val foreground = resolve(cell.style.foreground, ansi, defaultForeground).let { if (cell.style.dim) it.copy(alpha = 0.6f) else it }
                val background = resolve(cell.style.background, ansi, Color.Transparent)
                val painted = if (inverse) background.takeIf { it != Color.Transparent } ?: defaultBackground else foreground
                val fill = if (inverse) foreground else background
                val span = cellSpan(cell.style, painted)

                if (fill != fillColour) {
                    paint(column)
                    fillColour = fill
                    fillStart = column
                }

                if (span != runStyle) {
                    flush()
                    runStyle = span
                }

                run.append(codePointString(cell.codePoint))
            }

            flush()
        }

        paint(emulator.columns)
        drawText(measurer, text, Offset(0f, top), style, softWrap = false, maxLines = 1)
    }
}

private fun androidx.compose.ui.graphics.drawscope.DrawScope.drawCursor(
    emulator: Terminal,
    cellWidth: Float,
    cellHeight: Float,
    colour: Color,
    focused: Boolean,
    blinkOn: Boolean,
) {
    if (!terminalCursorShown(emulator.cursorVisible, focused, blinkOn)) {
        return
    }

    val origin = Offset(emulator.cursorColumn * cellWidth, emulator.cursorRow * cellHeight)

    if (focused) {
        drawRect(colour, origin, Size(cellWidth, cellHeight), alpha = CursorBlockAlpha)
    } else {
        drawRect(colour, origin, Size(cellWidth, cellHeight), style = Stroke(1.dp.toPx()))
    }
}

private fun cellSpan(style: CellStyle, color: Color): SpanStyle = SpanStyle(
    color = color,
    fontWeight = if (style.bold) FontWeight.Medium else null,
    fontStyle = if (style.italic) FontStyle.Italic else null,
    textDecoration = if (style.underline) TextDecoration.Underline else null,
)
