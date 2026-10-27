package mycelium.ui

import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.isShiftPressed
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.input.pointer.PointerEventType
import androidx.compose.ui.input.pointer.isSecondaryPressed
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics

@Stable
class RowMenuState {
    var open by mutableStateOf(false)
}

@Composable
fun rememberRowMenuState(): RowMenuState = remember { RowMenuState() }

fun Modifier.rowContextMenu(state: RowMenuState): Modifier =
    this
        .onKeyEvent { event ->
            val opensMenu = event.type == KeyEventType.KeyDown && (event.key == Key.Menu || (event.key == Key.F10 && event.isShiftPressed))

            if (opensMenu) {
                state.open = true
            }

            opensMenu
        }
        .pointerInput(state) { detectTapGestures(onLongPress = { state.open = true }) }
        .pointerInput(state) {
            awaitPointerEventScope {
                while (true) {
                    val event = awaitPointerEvent()

                    if (event.type == PointerEventType.Press && event.buttons.isSecondaryPressed) {
                        state.open = true
                    }
                }
            }
        }

@Composable
fun RowMenuPopup(state: RowMenuState, content: @Composable (close: () -> Unit) -> Unit) {
    if (state.open) {
        MenuPopup(onDismiss = { state.open = false }) {
            content { state.open = false }
        }
    }
}

@Composable
fun RowMenu(title: String, state: RowMenuState = rememberRowMenuState(), content: @Composable (close: () -> Unit) -> Unit) {
    Box {
        MyButton({ state.open = true }, icon = MyIcon.MoreVertical, description = "More for $title", size = ButtonSize.Sm, muted = true, selected = state.open)
        RowMenuPopup(state, content)
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

        FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            MyButton(onDismiss, text = "Cancel", muted = true)
            MyButton(onConfirm, text = confirm, variant = ButtonVariant.Danger)
        }
    }
}
