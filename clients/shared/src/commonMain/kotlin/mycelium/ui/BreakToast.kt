package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import mycelium.state.BreakClock
import mycelium.state.BreakPhase

@Composable
fun BreakToast(clock: BreakClock, minutes: Int, onSnooze: () -> Unit, onStartBreak: () -> Unit, onDismiss: () -> Unit, modifier: Modifier = Modifier) {
    if (clock.phase != BreakPhase.Due) {
        return
    }

    val colors = MyTheme.colors

    Column(
        modifier
            .widthIn(max = Tokens.Size.Menu * 1.4f)
            .background(colors.bgRaised, ControlShape)
            .border(Tokens.Size.Hairline, colors.borderControl, ControlShape)
            .padding(Tokens.Space.S3)
            .onPreviewKeyEvent { event ->
                if (event.type == KeyEventType.KeyDown && event.key == Key.Escape) {
                    onDismiss()
                    true
                } else {
                    false
                }
            }
            .semantics { liveRegion = LiveRegionMode.Polite },
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
            Icon(MyIcon.Cup, colors.accent, Tokens.Size.IconMd)
            Label("Time for a break", style = Tokens.Type.UiStrong)
        }
        Label("You have been at it for ${clock.activeMinutes} minutes. A $minutes minute break will do.", style = Tokens.Type.Meta, color = colors.textMuted)

        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5)) {
            MyButton(onStartBreak, text = "Start break", variant = ButtonVariant.Primary)
            MyButton(onSnooze, text = "Snooze", variant = ButtonVariant.Outline)
            MyButton(onDismiss, text = "Dismiss", muted = true)
        }
    }
}
