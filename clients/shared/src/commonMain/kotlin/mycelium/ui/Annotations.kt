package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.foundation.layout.Box
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.isCtrlPressed
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
)

val LocalAnnotate = staticCompositionLocalOf<AnnotateActions?> { null }

private val CommandKeys = setOf(Key.CtrlLeft, Key.CtrlRight, Key.MetaLeft, Key.MetaRight)

private enum class Intent { Attach, Ask }

@Composable
fun Annotatable(content: @Composable () -> Unit) {
    val actions = LocalAnnotate.current

    if (actions == null) {
        SelectionContainer(content = content)

        return
    }

    val colors = MyTheme.colors
    val clipboard = LocalClipboardManager.current
    var offered by remember { mutableStateOf<String?>(null) }
    var composing by remember { mutableStateOf<Pair<Intent, String>?>(null) }
    var copying by remember { mutableStateOf(false) }
    var commandHeld by remember { mutableStateOf(false) }
    var draft by remember { mutableStateOf("") }
    val toolbar = remember {
        object : TextToolbar {
            override val status: TextToolbarStatus get() = if (offered != null) TextToolbarStatus.Shown else TextToolbarStatus.Hidden

            override fun showMenu(rect: Rect, onCopyRequested: (() -> Unit)?, onPasteRequested: (() -> Unit)?, onCutRequested: (() -> Unit)?, onSelectAllRequested: (() -> Unit)?) {
                val before = clipboard.getText()
                onCopyRequested?.invoke()
                offered = clipboard.getText()?.text?.takeIf { it.isNotBlank() }

                if (before != null) {
                    clipboard.setText(before)
                }
            }

            override fun hide() {}
        }
    }

    Box(
        Modifier.onPreviewKeyEvent { event ->
            if (event.key in CommandKeys) {
                commandHeld = event.type == KeyEventType.KeyDown
            } else if (event.type == KeyEventType.KeyDown && event.key == Key.C && (commandHeld || event.isCtrlPressed || event.isMetaPressed)) {
                copying = true
            } else if (event.type == KeyEventType.KeyDown && event.key == Key.Escape && offered != null) {
                offered = null
            } else if (event.type == KeyEventType.KeyUp && event.key == Key.C && copying) {
                copying = false
                offered = clipboard.getText()?.text?.takeIf { it.isNotBlank() }
            }

            false
        },
    ) {
        CompositionLocalProvider(LocalTextToolbar provides toolbar) {
            SelectionContainer(content = content)
        }

        val quote = offered

        if (quote != null && composing == null) {
            Popup(popupPositionProvider = remember { AboveAnchor() }, onDismissRequest = { offered = null }, properties = PopupProperties(focusable = false)) {
                Row(
                    Modifier
                        .background(colors.bgRaised, FloatShape)
                        .border(Tokens.Size.Hairline, colors.border, FloatShape)
                        .padding(Tokens.Space.S0_5)
                        .semantics { contentDescription = "Selection actions" },
                    horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5),
                ) {
                    MyButton({ composing = Intent.Attach to quote; draft = ""; offered = null }, text = "Add to message", size = ButtonSize.Sm, muted = true)
                    MyButton({ composing = Intent.Ask to quote; draft = ""; offered = null }, text = "Ask in a side chat", size = ButtonSize.Sm, muted = true)
                }
            }
        }

        val pending = composing

        if (pending != null) {
            val (intent, selected) = pending

            fun finish() {
                if (intent == Intent.Attach) {
                    actions.onAttach(Annotation(selected, draft.trim()))
                } else if (draft.isNotBlank()) {
                    actions.onAsk(selected, draft.trim())
                }

                composing = null
            }

            Popup(alignment = Alignment.Center, onDismissRequest = { composing = null }, properties = PopupProperties(focusable = true)) {
                Column(
                    Modifier
                        .width(Tokens.Size.Popover)
                        .background(colors.bgRaised, FloatShape)
                        .border(Tokens.Size.Hairline, colors.border, FloatShape)
                        .padding(Tokens.Space.S3)
                        .semantics { contentDescription = if (intent == Intent.Attach) "Add to message" else "Ask in a side chat" },
                    verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                ) {
                    Label(annotationLabel(Annotation(selected, "")), style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 2)
                    TextInput(
                        draft,
                        { draft = it },
                        label = if (intent == Intent.Attach) "Comment" else "Question",
                        placeholder = if (intent == Intent.Attach) "Add a comment, or leave empty to quote only" else "What do you want to ask about this?",
                        onSubmit = ::finish,
                    )
                    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                        MyButton(::finish, text = if (intent == Intent.Attach) "Add" else "Ask", size = ButtonSize.Sm, variant = ButtonVariant.Outline)
                        MyButton({ composing = null }, text = "Cancel", size = ButtonSize.Sm, muted = true)
                    }
                }
            }
        }
    }
}

private class AboveAnchor : PopupPositionProvider {
    override fun calculatePosition(anchorBounds: IntRect, windowSize: IntSize, layoutDirection: LayoutDirection, popupContentSize: IntSize): IntOffset {
        val above = anchorBounds.top - popupContentSize.height
        val y = if (above >= 0) above else anchorBounds.bottom.coerceAtMost(windowSize.height - popupContentSize.height)
        val x = (anchorBounds.right - popupContentSize.width).coerceIn(0, maxOf(0, windowSize.width - popupContentSize.width))

        return IntOffset(x, y.coerceAtLeast(0))
    }
}
