package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import mycelium.api.ConversationUsage
import mycelium.api.MyceliumApi
import mycelium.state.formatCost
import mycelium.state.formatTokens
import mycelium.state.percentOf

fun contextLabel(usage: ConversationUsage): String? {
    val used = usage.contextUsed ?: return null
    val size = usage.contextSize ?: return "Context ${formatTokens(used)}, limit unknown"

    return "Context ${formatTokens(used)} of ${formatTokens(size)} (${percentOf(used, size)}%)"
}

fun spendLabel(usage: ConversationUsage): String? = when {
    usage.costUsd != null -> "Spent ${formatCost(usage.costUsd)}"
    usage.tokens > 0 -> "Billed ${formatTokens(usage.tokens)} tokens, cost not reported"
    else -> null
}

@Composable
fun ConversationUsageBar(api: MyceliumApi, id: String, refreshOn: Any?) {
    val usage by produceState<ConversationUsage?>(null, id, refreshOn) {
        value = runCatching { api.usageOf(id) }.getOrNull() ?: value
    }
    val current = usage ?: return
    val context = contextLabel(current)
    val spend = spendLabel(current)

    if (context == null && spend == null) {
        return
    }

    val colors = MyTheme.colors

    Row(
        Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S3),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (context != null) {
            Label(context, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
            val size = current.contextSize
            val used = current.contextUsed

            if (size != null && used != null) {
                DeterminateProgress(context, used.toFloat() / size, Modifier.width(Tokens.Size.ContextMeter))
            }
        }

        if (spend != null) {
            Label(spend, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
        }
    }
}
