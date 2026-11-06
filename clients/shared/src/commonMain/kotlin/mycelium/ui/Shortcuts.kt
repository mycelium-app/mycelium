package mycelium.ui

import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEvent
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.isCtrlPressed
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.type
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.SharedFlow

object TerminalShortcut {
    private val pressed = MutableSharedFlow<Unit>(extraBufferCapacity = 1)

    val presses: SharedFlow<Unit> = pressed

    fun handle(event: KeyEvent): Boolean {
        val toggles = event.type == KeyEventType.KeyDown && event.isCtrlPressed && event.key == Key.Grave

        if (toggles) {
            press()
        }

        return toggles
    }

    fun press() {
        pressed.tryEmit(Unit)
    }
}
