package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import kotlin.math.roundToInt
import mycelium.state.AgentAccount
import mycelium.state.runsOutBeforeReset
import mycelium.state.usageProjection
import mycelium.state.windowName

@Composable
fun UsageCard(accounts: List<Pair<AgentAccount, Boolean>>, modifier: Modifier = Modifier) {
    val colors = MyTheme.colors

    BoxWithConstraints(modifier) {
        Column(
            Modifier
                .fillMaxWidth()
                .heightIn(max = maxHeight / 2)
                .background(colors.bgRaised, BlockShape)
                .border(Tokens.Size.Hairline, colors.border, BlockShape)
                .verticalScroll(rememberScrollState()),
        ) {
            Label("Usage", Modifier.padding(Tokens.Space.S1_5).semantics { heading() }, Tokens.Type.UiStrong)

            val reported = accounts.filter { (account, _) -> account.windows.isNotEmpty() }

            if (reported.isEmpty()) {
                Hairline()
                Label("No account has reported usage yet.", Modifier.padding(Tokens.Space.S1_5), Tokens.Type.Meta, colors.textMuted)
            }

            for ((account, thisChat) in reported) {
                Hairline()

                UsageBlock(account, thisChat)
            }
        }
    }
}

@Composable
private fun UsageBlock(account: AgentAccount, thisChat: Boolean) {
    val colors = MyTheme.colors
    val platform = LocalPlatform.current
    val localDateTime: (Long) -> String? = { millis -> platform?.localDateTime(millis) }

    Column(Modifier.fillMaxWidth().padding(Tokens.Space.S1_5), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
            Icon(agentIcon(account.agent), colors.text, Tokens.Size.IconMd)
            Label(account.label.substringAfter('@'), Modifier.weight(1f, fill = false), Tokens.Type.UiStrong)

            if (thisChat) {
                Tag("This chat")
            }
        }

        for (window in account.windows) {
            val percent = window.usedPercent.roundToInt().coerceIn(0, 100)
            val name = windowName(window.name).replaceFirstChar { it.uppercase() }
            val projection = usageProjection(window, localDateTime)

            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Label(name, Modifier.weight(1f), Tokens.Type.Meta, maxLines = 1)
                    Label("$percent% used", style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
                }

                Meter(percent, "$name $percent% used", attention = runsOutBeforeReset(window))

                if (projection.isNotEmpty()) {
                    Label(projection, style = Tokens.Type.Meta, color = colors.textMuted)
                }
            }
        }
    }
}
