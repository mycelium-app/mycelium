package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.ui.unit.dp
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import mycelium.api.ChatToolInfo
import mycelium.api.ChatToolServer
import mycelium.api.IntegrationPolicy
import mycelium.api.PolicyLevel

private const val Inherit = "Inherit"
private const val Yes = "Yes"
private const val No = "No"

enum class PolicySource(val label: String) {
    Default("integration default"),
    Type("tool type"),
    Tool("this tool"),
}

data class ResolvedPolicy(val value: Boolean, val source: PolicySource)

fun resolvePolicy(level: PolicyLevel, tool: ChatToolInfo, fallback: Boolean): ResolvedPolicy {
    level.tools[tool.name]?.let { return ResolvedPolicy(it, PolicySource.Tool) }

    val byType = if (tool.kind == "read") level.read else level.write
    byType?.let { return ResolvedPolicy(it, PolicySource.Type) }

    return ResolvedPolicy(level.all ?: fallback, PolicySource.Default)
}

private fun choice(value: Boolean?): String = when (value) {
    null -> Inherit
    true -> Yes
    false -> No
}

private fun parse(option: String): Boolean? = when (option) {
    Yes -> true
    No -> false
    else -> null
}

@Composable
private fun PolicyChoice(label: String, value: Boolean?, onChange: (Boolean?) -> Unit, hint: String? = null) {
    val colors = MyTheme.colors

    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        Column(Modifier.weight(1f)) {
            Label(label, maxLines = 1)
            hint?.let { Label(it, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1) }
        }

        Segmented(listOf(Inherit, Yes, No), choice(value), { onChange(parse(it)) }, label)
    }
}

@Composable
private fun PolicySection(title: String, question: String, level: PolicyLevel, tools: List<ChatToolInfo>, fallback: Boolean, onChange: (PolicyLevel) -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        Label(title, Modifier.semantics { heading() }, Tokens.Type.UiStrong)
        PolicyChoice("$title: all tools", level.all, { onChange(level.copy(all = it)) }, "$question every tool unless a narrower rule says otherwise")
        PolicyChoice("$title: read tools", level.read, { onChange(level.copy(read = it)) })
        PolicyChoice("$title: write tools", level.write, { onChange(level.copy(write = it)) })

        for (tool in tools) {
            val resolved = resolvePolicy(level, tool, fallback)
            val hint = if (resolved.source == PolicySource.Tool) "Set for this tool" else "Inherited from the ${resolved.source.label}: ${if (resolved.value) "yes" else "no"}"
            val override = level.tools[tool.name]
            PolicyChoice("$title: ${tool.name}", override, { chosen -> onChange(level.copy(tools = if (chosen == null) level.tools - tool.name else level.tools + (tool.name to chosen))) }, hint)
        }
    }
}

@Composable
fun IntegrationPolicyDialog(server: ChatToolServer, onSave: (IntegrationPolicy) -> Unit, onDismiss: () -> Unit) {
    val colors = MyTheme.colors
    var policy by remember { mutableStateOf(server.policy) }

    Dialog(onDismiss) {
        Label("Approvals for ${server.name}", Modifier.semantics { heading() }, Tokens.Type.Title)

        if (server.locked) {
            Label("This integration always asks before a call and always shows its results for review. That cannot be changed.", color = colors.textMuted)
            MyButton(onDismiss, text = "Close", variant = ButtonVariant.Outline)
            return@Dialog
        }

        Label("A rule on a tool overrides its type, and a rule on a type overrides the integration default.", style = Tokens.Type.Meta, color = colors.textMuted)

        Column(Modifier.heightIn(max = 480.dp).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4)) {
            PolicySection("Ask before", "Ask before running", policy.ask, server.tools, !server.alwaysAllow) { policy = policy.copy(ask = it) }
            PolicySection("Review", "Hold the result for your review for", policy.review, server.tools, false) { policy = policy.copy(review = it) }
        }

        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            MyButton({ onSave(policy) }, text = "Save approvals", variant = ButtonVariant.Outline)
            MyButton(onDismiss, text = "Cancel", muted = true)
        }
    }
}

private val Months = listOf("Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec")

fun expiryNote(server: ChatToolServer): String? {
    val date = server.tokenExpiresAt?.take(10)?.split("-")?.takeIf { it.size == 3 } ?: return null
    val month = date[1].toIntOrNull()?.let { Months.getOrNull(it - 1) } ?: return null
    val day = date[2].toIntOrNull() ?: return null
    val when_ = "$day $month ${date[0]}"

    return when (server.status) {
        "expired" -> "Expired $when_. Replace the token to reconnect."
        "expires_soon" -> "Expires $when_. Replace the token before then."
        else -> "Expires $when_."
    }
}
