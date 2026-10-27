package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.compositionLocalOf
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

internal class PausedWorkflow(val agent: String, val actions: LimitActions)

internal val LocalPausedWorkflow = compositionLocalOf<PausedWorkflow?> { null }

@Composable
fun UsageLimitCard(limit: UsageLimitNote, agent: String, actions: LimitActions?, said: String? = null) {
    val colors = MyTheme.colors

    Column(
        Modifier
            .fillMaxWidth()
            .background(colors.bgRaised, FloatShape)
            .border(Tokens.Size.Hairline, colors.border, FloatShape)
            .padding(Tokens.Space.S3)
            .semantics { contentDescription = "Paused at a usage limit" },
    ) {
        LimitNote(limit, agent, actions, said)
    }
}

@Composable
internal fun LimitNote(limit: UsageLimitNote, agent: String, actions: LimitActions?, said: String? = null) {
    val colors = MyTheme.colors
    val platform = LocalPlatform.current
    val time: (String) -> String = { iso -> clockTime(iso) { millis -> platform?.localDateTime(millis) } }
    val reset = limit.resetsAt?.let(time)
    val name = "${agentName(agent).ifEmpty { "The agent" }}'s ${limitName(limit.window)}"
    val headline = if (reset != null) "Paused until $name resets at $reset." else "Paused at $name. Mycelium does not know when it resets."
    val resumeTime = limit.resetsAt?.let(::resumeTimeAfter)?.let(time)

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.Top) {
            Icon(MyIcon.Clock, colors.statusAttention, Tokens.Size.IconSm)
            Label(headline)
        }

        if (said != null) {
            Label("${agentName(agent).ifEmpty { "The agent" }} said: $said", style = Tokens.Type.Meta, color = colors.textMuted)
        }

        if (actions != null) {
            val pending = actions.resumeAt?.let(time)

            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                if (pending != null) {
                    Label("Resumes automatically at $pending.", color = colors.textMuted)
                    MyButton(actions.onCancelResume, text = "Cancel", description = "Cancel the automatic resume", size = ButtonSize.Sm, muted = true)
                } else if (resumeTime != null) {
                    MyButton(actions.onResume, text = "Resume at $resumeTime", description = "Resume this conversation at $resumeTime", size = ButtonSize.Sm, variant = ButtonVariant.Outline)
                }

                MyButton(actions.onRetryNow, text = "Retry now", description = "Retry this conversation now", size = ButtonSize.Sm, variant = ButtonVariant.Outline)
            }
        }
    }
}
