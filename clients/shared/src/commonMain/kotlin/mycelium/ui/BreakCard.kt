package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import kotlin.time.Duration.Companion.milliseconds
import mycelium.state.BreakClock
import mycelium.state.BreakPhase
import mycelium.state.countdownText

@Composable
fun BreakCard(clock: BreakClock, minutes: Int, onSnooze: () -> Unit, onStartBreak: () -> Unit, modifier: Modifier = Modifier) {
    if (clock.phase != BreakPhase.Due) {
        return
    }

    val colors = MyTheme.colors

    Column(
        modifier
            .fillMaxWidth()
            .background(colors.bgRaised, BlockShape)
            .border(Tokens.Size.Hairline, colors.border, BlockShape)
            .padding(Tokens.Space.S3),
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5),
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
            Icon(MyIcon.Cup, colors.accent, Tokens.Size.IconSm)
            Label("Time for a break", Modifier.semantics { heading(); liveRegion = LiveRegionMode.Polite }, Tokens.Type.UiStrong)
        }

        Label("${countdownText(clock.overdueMillis.milliseconds)} overdue", style = Tokens.Type.Title.copy(fontFamily = Tokens.Type.Code.fontFamily))
        Label("A $minutes minute break will do.", style = Tokens.Type.Meta, color = colors.textMuted)

        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            MyButton(onStartBreak, text = "Start break", variant = ButtonVariant.Primary)

            if (!clock.snoozed) {
                MyButton(onSnooze, text = "Snooze", variant = ButtonVariant.Outline)
            }
        }
    }
}
