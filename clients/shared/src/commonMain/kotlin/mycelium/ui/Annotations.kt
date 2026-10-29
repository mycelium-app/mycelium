@file:Suppress("INVISIBLE_MEMBER", "INVISIBLE_REFERENCE")

package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.text.selection.SelectionManager
import androidx.compose.foundation.text.selection.SelectionState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.foundation.layout.Box
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.isCtrlPressed
import androidx.compose.ui.input.key.isShiftPressed
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.input.key.isMetaPressed
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.type
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.key.onPreviewKeyEvent
import mycelium.state.annotationLabel
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.isSpecified
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalTextToolbar
import androidx.compose.ui.platform.TextToolbar
import androidx.compose.ui.platform.TextToolbarStatus
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntRect
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.window.Popup
import androidx.compose.ui.window.PopupPositionProvider
import androidx.compose.ui.window.PopupProperties
import mycelium.state.Annotation

class AnnotateActions(
    val onAttach: (Annotation) -> Unit,
    val onAsk: (quote: String, question: String) -> Unit,
    val withComment: Boolean = false,
)

val LocalAnnotate = staticCompositionLocalOf<AnnotateActions?> { null }

@Composable
fun Annotatable(content: @Composable () -> Unit) {
    val actions = LocalAnnotate.current

    if (actions == null) {
        SelectionContainer(content = content)

        return
    }

    val colors = MyTheme.colors
    val clipboard = LocalClipboardManager.current
    val state = remember { SelectionState() }
    var manager by remember { mutableStateOf<SelectionManager?>(null) }
    var offered by remember { mutableStateOf<String?>(null) }
    var asking by remember { mutableStateOf<String?>(null) }
    var commenting by remember { mutableStateOf<String?>(null) }
    var draft by remember { mutableStateOf("") }
    val selected = manager?.takeIf { it.selection != null }?.getSelectedText()?.text?.takeIf { it.isNotBlank() }

    LaunchedEffect(selected) {
        if (selected != null) {
            offered = selected
        }
    }

    val toolbar = remember {
        object : TextToolbar {
            override val status: TextToolbarStatus get() = TextToolbarStatus.Hidden

            override fun showMenu(rect: Rect, onCopyRequested: (() -> Unit)?, onPasteRequested: (() -> Unit)?, onCutRequested: (() -> Unit)?, onSelectAllRequested: (() -> Unit)?) {}

            override fun hide() {}
        }
    }

    fun ask(quote: String) {
        asking = quote
        draft = ""
    }

    fun cite(quote: String) {
        if (actions.withComment) {
            commenting = quote
            draft = ""

            return
        }

        actions.onAttach(Annotation(quote, ""))
        offered = null
        state.clear()
    }

    Box(
        Modifier.onPreviewKeyEvent { event ->
            val command = event.isCtrlPressed || event.isMetaPressed
            val quote = offered

            if (event.type == KeyEventType.KeyDown && event.key == Key.Escape && quote != null) {
                offered = null
                state.clear()

                true
            } else if (event.type == KeyEventType.KeyDown && event.key == Key.A && command && event.isShiftPressed && quote != null) {
                cite(quote)

                true
            } else {
                false
            }
        },
    ) {
        CompositionLocalProvider(LocalTextToolbar provides toolbar) {
            SelectionContainer(Modifier, state, { manager = it }, content)
        }

        val quote = offered

        if (quote != null && asking == null && commenting == null) {
            val start = manager?.startHandlePosition
            val end = manager?.endHandlePosition
            val top = listOfNotNull(start, end).filter { it.isSpecified }.minOfOrNull { it.y }?.toInt() ?: 0

            Popup(popupPositionProvider = remember(top) { AboveSelection(top) }, onDismissRequest = { offered = null }, properties = PopupProperties(focusable = false)) {
                Row(
                    Modifier
                        .background(colors.bgRaised, FloatShape)
                        .border(Tokens.Size.Hairline, colors.border, FloatShape)
                        .padding(Tokens.Space.S0_5)
                        .semantics { contentDescription = "Selection actions" },
                    horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    MyButton({ cite(quote) }, text = if (actions.withComment) "Add to message" else "Cite", description = if (actions.withComment) "Add to message, Ctrl+Shift+A" else "Cite, Ctrl+Shift+A", size = ButtonSize.Sm, muted = true)
                    MyButton({ ask(quote) }, text = "Ask in a side chat", size = ButtonSize.Sm, muted = true)
                    MyButton({ clipboard.setText(AnnotatedString(quote)); offered = null; state.clear() }, text = "Copy", size = ButtonSize.Sm, muted = true)
                }
            }
        }

        val noting = commenting

        if (noting != null) {
            fun finishNote() {
                actions.onAttach(Annotation(noting, draft.trim()))
                commenting = null
                offered = null
                state.clear()
            }

            Popup(alignment = Alignment.Center, onDismissRequest = { commenting = null }, properties = PopupProperties(focusable = true)) {
                Column(
                    Modifier
                        .width(Tokens.Size.Popover)
                        .background(colors.bgRaised, FloatShape)
                        .border(Tokens.Size.Hairline, colors.border, FloatShape)
                        .padding(Tokens.Space.S3)
                        .semantics { contentDescription = "Add to message" },
                    verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                ) {
                    Label(annotationLabel(Annotation(noting, "")), style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 2)
                    TextInput(draft, { draft = it }, label = "Comment", placeholder = "Add a comment, or leave empty to quote only", onSubmit = ::finishNote)
                    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                        MyButton(::finishNote, text = "Add", size = ButtonSize.Sm, variant = ButtonVariant.Outline)
                        MyButton({ commenting = null }, text = "Cancel", size = ButtonSize.Sm, muted = true)
                    }
                }
            }
        }

        val pending = asking

        if (pending != null) {
            val quote = pending

            fun finish() {
                if (draft.isNotBlank()) {
                    actions.onAsk(quote, draft.trim())
                }

                asking = null
                offered = null
                state.clear()
            }

            Popup(alignment = Alignment.Center, onDismissRequest = { asking = null }, properties = PopupProperties(focusable = true)) {
                Column(
                    Modifier
                        .width(Tokens.Size.Popover)
                        .background(colors.bgRaised, FloatShape)
                        .border(Tokens.Size.Hairline, colors.border, FloatShape)
                        .padding(Tokens.Space.S3)
                        .semantics { contentDescription = "Ask in a side chat" },
                    verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                ) {
                    Label(annotationLabel(Annotation(quote, "")), style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 2)
                    TextInput(
                        draft,
                        { draft = it },
                        label = "Question",
                        placeholder = "What do you want to ask about this?",
                        onSubmit = ::finish,
                    )
                    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                        MyButton(::finish, text = "Ask", size = ButtonSize.Sm, variant = ButtonVariant.Outline)
                        MyButton({ asking = null }, text = "Cancel", size = ButtonSize.Sm, muted = true)
                    }
                }
            }
        }
    }
}

private class AboveSelection(private val top: Int) : PopupPositionProvider {
    override fun calculatePosition(anchorBounds: IntRect, windowSize: IntSize, layoutDirection: LayoutDirection, popupContentSize: IntSize): IntOffset {
        val above = anchorBounds.top + top - popupContentSize.height - Gap
        val y = if (above >= 0) above else anchorBounds.top + top + Gap
        val x = anchorBounds.left.coerceIn(0, maxOf(0, windowSize.width - popupContentSize.width))

        return IntOffset(x, y.coerceAtMost(windowSize.height - popupContentSize.height).coerceAtLeast(0))
    }

    private companion object {
        const val Gap = 28
    }
}
