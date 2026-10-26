package mycelium.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
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

    var open by remember(id) { mutableStateOf(false) }
    val compactContext = current.contextUsed?.let { used ->
        val size = current.contextSize

        if (size != null && size > 0) {
            "${percentOf(used, size)}%"
        } else {
            formatTokens(used)
        }
    }
    val summary = listOfNotNull(compactContext, current.costUsd?.let { formatCost(it) }).joinToString("  ").ifEmpty { "Usage" }

    Box {
        MyButton(
            { open = !open },
            text = summary,
            icon = MyIcon.Gauge,
            description = "Conversation usage",
            size = ButtonSize.Sm,
            muted = true,
            selected = open,
            tooltip = listOfNotNull(context, spend).joinToString("; "),
        )

        if (open) {
            MenuPopup(onDismiss = { open = false }) {
                context?.let { MenuLabel(it) }
                spend?.let { MenuLabel(it) }
            }
        }
    }
}
