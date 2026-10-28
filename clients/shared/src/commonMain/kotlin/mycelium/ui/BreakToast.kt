package mycelium.ui

import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
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

    Toast(
        MyIcon.Cup,
        colors.accent,
        onDismiss,
        modifier.semantics { liveRegion = LiveRegionMode.Polite },
        actions = {
            MyButton(onStartBreak, text = "Start break", variant = ButtonVariant.Primary)
            MyButton(onSnooze, text = "Snooze", variant = ButtonVariant.Outline)
        },
    ) {
        Label("Time for a break", style = Tokens.Type.UiStrong)
        Label("You have been at it for ${clock.activeMinutes} minutes. A $minutes minute break will do.", style = Tokens.Type.Meta, color = colors.textMuted)
    }
}
