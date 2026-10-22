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
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
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
import mycelium.api.ApiException
import mycelium.api.Conversation
import mycelium.state.AppState
import mycelium.state.CellStyle
import mycelium.state.Store
import mycelium.state.Terminal
import mycelium.state.TerminalColor
import mycelium.state.TerminalKey
import mycelium.state.codePointString
import mycelium.state.projectName
import mycelium.state.terminalControlInput
import mycelium.state.terminalKeyInput
import mycelium.state.xtermRgb
import androidx.compose.ui.unit.dp

@Composable
fun TerminalPanel(store: Store, state: AppState, conversation: Conversation, modifier: Modifier = Modifier) {
    val colors = MyTheme.colors

    Column(modifier.background(colors.bgPanel)) {
        Bar {
            BarTitle("Terminal")
            Label("tmux  ·  ${projectName(conversation.cwd)}", style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
        }

        TerminalView(store, state, conversation, Modifier.weight(1f).fillMaxWidth())
    }
}

@Composable
private fun TerminalView(store: Store, state: AppState, conversation: Conversation, modifier: Modifier) {
    val colors = MyTheme.colors
    val touch = LocalTouch.current
    val density = LocalDensity.current
    val measurer = rememberTextMeasurer()
    val style = Tokens.Type.Code.themed()
    val cellWidth = remember(style, measurer) { measurer.measure("M".repeat(CellProbeLength), style).size.width.toFloat() / CellProbeLength }
    val cellHeight = with(density) { style.lineHeight.toPx() }
    val ansi = ansiColours(colors)
    val source = remember { MutableInteractionSource() }
    val focused = source.collectIsFocusedAsState()
    val focusRequester = remember { FocusRequester() }
    var attempt by remember(conversation.id) { mutableIntStateOf(0) }
    var area by remember { mutableStateOf(IntSize.Zero) }
    var terminalId by remember(conversation.id, attempt) { mutableStateOf<String?>(null) }
    var failure by remember(conversation.id, attempt) { mutableStateOf<String?>(null) }
    val columns = if (cellWidth > 0f) (area.width / cellWidth).toInt() else 0
    val rows = if (cellHeight > 0f) (area.height / cellHeight).toInt() else 0
    val emulator = remember(conversation.id, attempt) { Terminal(DefaultColumns, DefaultRows) }
    var revision by remember(emulator) { mutableIntStateOf(0) }
    val sentSize = remember(emulator) { intArrayOf(0, 0) }
    val consumed = remember(emulator) { intArrayOf(0) }
    val id = terminalId
    val exited = id != null && id in state.exitedTerminals
    val feed = id?.let { state.terminalOutput[it] }

    LaunchedEffect(emulator, columns > 0 && rows > 0) {
        if (columns > 0 && rows > 0 && terminalId == null) {
            try {
                terminalId = store.api.startTerminal(conversation.id, columns, rows).terminalId
                sentSize[0] = columns
                sentSize[1] = rows
            } catch (problem: ApiException) {
                failure = problem.message
            } catch (_: Exception) {
                failure = "Could not reach the control plane. Check your connection and try again."
            }
        }
    }

    LaunchedEffect(id, columns, rows) {
        if (columns > 0 && rows > 0) {
            emulator.resize(columns, rows)
            revision++

            if (id != null && (sentSize[0] != columns || sentSize[1] != rows)) {
                sentSize[0] = columns
                sentSize[1] = rows
                store.act { store.api.resizeTerminal(id, columns, rows) }
            }
        }
    }

    LaunchedEffect(id, feed?.received) {
        if (feed != null) {
            feed.unseen(consumed[0]).forEach { emulator.feed(Base64.decode(it)) }
            consumed[0] = feed.received
            revision++
        }
    }

    LaunchedEffect(id) {
        if (id != null && !touch) {
            focusRequester.requestFocus()
        }
    }

    DisposableEffect(id) {
        onDispose {
            if (id != null) {
                store.stopTerminal(id)
            }
        }
    }

    Column(modifier) {
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
                .pointerInput(Unit) { detectTapGestures { focusRequester.requestFocus() } }
                .semantics { contentDescription = "Terminal for ${projectName(conversation.cwd)}. Press Shift and Tab to leave it." },
        ) {
            Canvas(Modifier.fillMaxSize()) {
                revision.let { drawEmulator(emulator, measurer, style, cellWidth, cellHeight, ansi, colors.text, colors.bgContent, focused.value) }
            }
        }

        if (failure != null || exited) {
            Row(
                Modifier.fillMaxWidth().background(colors.bgChrome).padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S1),
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Label(failure ?: "The terminal closed. Open it again to carry on.", Modifier.weight(1f), Tokens.Type.Meta, colors.textMuted)
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

    if (codePoint < ' '.code || codePoint == 0x7F) {
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
    focused: Boolean,
) {
    for (row in 0 until emulator.rows) {
        val top = row * cellHeight
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
                val atCursor = focused && emulator.cursorVisible && row == emulator.cursorRow && column == emulator.cursorColumn
                val inverse = cell.style.inverse != atCursor
                val foreground = resolve(cell.style.foreground, ansi, defaultForeground).let { if (cell.style.dim) it.copy(alpha = 0.6f) else it }
                val background = resolve(cell.style.background, ansi, Color.Transparent)
                val painted = if (inverse) background.takeIf { it != Color.Transparent } ?: defaultBackground else foreground
                val fill = if (inverse) foreground else background
                val span = cellSpan(cell.style, painted)

                if (fill != Color.Transparent) {
                    drawRect(fill, Offset(column * cellWidth, top), Size(cellWidth, cellHeight))
                }

                if (span != runStyle) {
                    flush()
                    runStyle = span
                }

                run.append(codePointString(cell.codePoint))
            }

            flush()
        }

        drawText(measurer, text, Offset(0f, top), style, softWrap = false, maxLines = 1)
    }

    if (!focused && emulator.cursorVisible) {
        drawRect(defaultForeground, Offset(emulator.cursorColumn * cellWidth, emulator.cursorRow * cellHeight), Size(cellWidth, cellHeight), style = Stroke(1.dp.toPx()))
    }
}

private fun cellSpan(style: CellStyle, color: Color): SpanStyle = SpanStyle(
    color = color,
    fontWeight = if (style.bold) FontWeight.Medium else null,
    fontStyle = if (style.italic) FontStyle.Italic else null,
    textDecoration = if (style.underline) TextDecoration.Underline else null,
)
