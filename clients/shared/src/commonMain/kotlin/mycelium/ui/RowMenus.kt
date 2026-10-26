package mycelium.ui

import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.Row
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.input.pointer.PointerEventType
import androidx.compose.ui.input.pointer.isSecondaryPressed
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics

@Stable
class RowMenuState {
    var open by mutableStateOf(false)
    var hovered by mutableStateOf(false)
    var focused by mutableStateOf(false)

    val revealed: Boolean get() = open || hovered || focused
}

@Composable
fun rememberRowMenuState(): RowMenuState = remember { RowMenuState() }

fun Modifier.rowContextMenu(state: RowMenuState): Modifier =
    this
        .onFocusChanged { state.focused = it.hasFocus }
        .pointerInput(state) { detectTapGestures(onLongPress = { state.open = true }) }
        .pointerInput(state) {
            awaitPointerEventScope {
                while (true) {
                    val event = awaitPointerEvent()

                    when (event.type) {
                        PointerEventType.Enter -> state.hovered = true
                        PointerEventType.Exit -> state.hovered = false
                        PointerEventType.Press -> if (event.buttons.isSecondaryPressed) {
                            state.open = true
                        }
                    }
                }
            }
        }

@Composable
fun RowMenu(title: String, state: RowMenuState, revealOnHover: Boolean = false, content: @Composable (close: () -> Unit) -> Unit) {
    val touch = LocalTouch.current

    Box(Modifier.alpha(if (!revealOnHover || state.revealed) 1f else 0f)) {
        if (!(revealOnHover && touch)) {
            MyButton({ state.open = true }, icon = MyIcon.MoreVertical, description = if (revealOnHover) "Actions for $title" else "More for $title", size = ButtonSize.Sm, muted = true, selected = state.open)
        }

        if (state.open) {
            MenuPopup(onDismiss = { state.open = false }) {
                content { state.open = false }
            }
        }
    }
}

@Composable
fun BackButton(to: String, onClick: () -> Unit, modifier: Modifier = Modifier, showText: Boolean = false) {
    MyButton(onClick, text = if (showText) to else null, icon = MyIcon.ChevronLeft, description = "Back to $to", muted = true, modifier = modifier)
}

@Composable
fun ConfirmDialog(title: String, detail: String, confirm: String, onDismiss: () -> Unit, onConfirm: () -> Unit) {
    Dialog(onDismiss = onDismiss) {
        Label(title, Modifier.semantics { heading() }, Tokens.Type.Title)
        Label(detail, style = Tokens.Type.Meta, color = MyTheme.colors.textMuted)

        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End)) {
            MyButton(onDismiss, text = "Cancel", muted = true)
            MyButton(onConfirm, text = confirm, variant = ButtonVariant.Danger)
        }
    }
}
