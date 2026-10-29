package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.focusable
import androidx.compose.foundation.gestures.Orientation
import androidx.compose.foundation.gestures.draggable
import androidx.compose.foundation.gestures.rememberDraggableState
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.requiredHeight
import androidx.compose.foundation.layout.requiredWidth
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.input.pointer.pointerHoverIcon
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp

@Composable
fun ResizeHandle(description: String, orientation: Orientation = Orientation.Horizontal, onMove: (Dp) -> Unit) {
    val colors = MyTheme.colors
    val density = LocalDensity.current
    val handle = remember { MutableInteractionSource() }
    val focused by handle.collectIsFocusedAsState()
    val vertical = orientation == Orientation.Vertical

    Box(
        Modifier
            .then(if (vertical) Modifier.fillMaxWidth().height(Tokens.Size.Hairline) else Modifier.fillMaxHeight().width(Tokens.Size.Hairline))
            .background(if (focused) colors.focus else colors.border)
            .semantics { contentDescription = description },
    ) {
        Box(
            Modifier
                .then(if (vertical) Modifier.fillMaxWidth().requiredHeight(Tokens.Space.S2) else Modifier.fillMaxHeight().requiredWidth(Tokens.Space.S2))
                .pointerHoverIcon(if (vertical) VerticalResizeIcon else HorizontalResizeIcon)
                .draggable(rememberDraggableState { delta -> onMove(with(density) { delta.toDp() }) }, orientation)
                .onKeyEvent { event ->
                    val step = when (event.key) {
                        Key.DirectionLeft -> if (vertical) null else -Tokens.Space.S4
                        Key.DirectionRight -> if (vertical) null else Tokens.Space.S4
                        Key.DirectionUp -> if (vertical) -Tokens.Space.S4 else null
                        Key.DirectionDown -> if (vertical) Tokens.Space.S4 else null
                        else -> null
                    }

                    if (event.type == KeyEventType.KeyDown && step != null) {
                        onMove(step)
                    }

                    step != null
                }
                .focusable(interactionSource = handle),
        )
    }
}
