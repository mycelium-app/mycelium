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
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import mycelium.state.UsageLimitNote
import mycelium.state.agentName
import mycelium.state.clockTime
import mycelium.state.limitName
import mycelium.state.resumeTimeAfter

data class LimitActions(
    val resumeAt: String?,
    val onResume: () -> Unit,
    val onRetryNow: () -> Unit,
    val onCancelResume: () -> Unit,
)

@Composable
fun UsageLimitCard(limit: UsageLimitNote, agent: String, actions: LimitActions?) {
    val colors = MyTheme.colors
    val platform = LocalPlatform.current
    val time: (String) -> String = { iso -> clockTime(iso) { millis -> platform?.localDateTime(millis) } }
    val reset = limit.resetsAt?.let(time)
    val headline = "${agentName(agent).ifEmpty { "The agent" }}'s ${limitName(limit.window)} is reached." + (if (reset != null) " It resets at $reset." else " Mycelium does not know when it resets.")
    val resumeTime = limit.resetsAt?.let(::resumeTimeAfter)?.let(time)

    Column(
        Modifier
            .fillMaxWidth()
            .background(colors.bgRaised, FloatShape)
            .border(Tokens.Size.Hairline, colors.border, FloatShape)
            .padding(Tokens.Space.S3)
            .semantics { contentDescription = "Usage limit reached" },
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.Top) {
            Icon(MyIcon.Clock, colors.statusAttention, Tokens.Size.IconSm)
            Label(headline)
        }

        if (actions != null) {
            val pending = actions.resumeAt?.let(time)

            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                if (pending != null) {
                    Label("Resumes at $pending.", color = colors.textMuted)
                    MyButton(actions.onCancelResume, text = "Cancel", description = "Cancel the automatic resume", size = ButtonSize.Sm, muted = true)
                } else if (resumeTime != null) {
                    MyButton(actions.onResume, text = "Resume at $resumeTime", description = "Resume this conversation at $resumeTime", size = ButtonSize.Sm, variant = ButtonVariant.Outline)
                }

                MyButton(actions.onRetryNow, text = "Retry now", description = "Retry this conversation now", size = ButtonSize.Sm, variant = ButtonVariant.Outline)
            }
        }
    }
}
