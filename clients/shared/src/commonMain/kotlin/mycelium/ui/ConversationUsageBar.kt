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
        attempt { value = api.usageOf(id) }
    }
    val current = usage ?: return
    val context = contextLabel(current)
    val spend = spendLabel(current)

    if (context == null && spend == null) {
        return
    }

    var open by remember(id) { mutableStateOf(false) }
    val size = current.contextSize?.takeIf { it > 0 }
    val compactContext = current.contextUsed?.let { used -> if (size != null) "${percentOf(used, size)}%" else formatTokens(used) }
    val cost = current.costUsd?.let { formatCost(it) }
    val named = listOfNotNull(compactContext?.let { if (size != null) "$it of the context" else "$it used" }, cost).joinToString(", ")

    Box {
        MyButton(
            { open = !open },
            text = compactContext ?: cost ?: "Usage",
            icon = MyIcon.Gauge,
            description = if (named.isEmpty()) "Conversation usage" else "Conversation usage: $named",
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
