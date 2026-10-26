package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.runtime.Composable
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

private const val ZulipReason = "Always on for Zulip, to keep to its data retention and visibility rules. Messages can be private or in channels the agent should not read, so you see every request and every result first."

private fun builtIn(server: ChatToolServer): Boolean = server.kind != "mcp" && server.kind != "litellm"

private fun tabbed(tools: List<ChatToolInfo>, kind: String): List<ChatToolInfo> = tools.filter { (it.kind == "read") == (kind == "read") }

@Composable
private fun PolicySection(title: String, question: String, level: PolicyLevel, server: ChatToolServer, fallback: Boolean, writesAlwaysAsk: Boolean, onChange: (PolicyLevel) -> Unit) {
    val colors = MyTheme.colors

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        Label(title, Modifier.semantics { heading() }, Tokens.Type.UiStrong)
        PolicyChoice("$title: all tools", level.all, { onChange(level.copy(all = it)) }, "$question every tool unless a narrower rule says otherwise")
        PolicyChoice("$title: read tools", level.read, { onChange(level.copy(read = it)) })

        if (writesAlwaysAsk) {
            Label("Write tools always ask first.", style = Tokens.Type.Meta, color = colors.textMuted)
        } else {
            PolicyChoice("$title: write tools", level.write, { onChange(level.copy(write = it)) })
        }

        for (kind in listOf("read", "write")) {
            val group = tabbed(server.tools, kind)

            if (group.isEmpty() || (writesAlwaysAsk && kind == "write")) {
                continue
            }

            Label(if (kind == "read") "Read tools" else "Write tools", style = Tokens.Type.MetaStrong, color = colors.textMuted)

            for (tool in group) {
                val resolved = resolvePolicy(level, tool, fallback)
                val hint = if (resolved.source == PolicySource.Tool) "Set for this tool" else "Inherited from the ${resolved.source.label}: ${if (resolved.value) "yes" else "no"}"
                val override = level.tools[tool.name]
                PolicyChoice("$title: ${tool.name}", override, { chosen -> onChange(level.copy(tools = if (chosen == null) level.tools - tool.name else level.tools + (tool.name to chosen))) }, hint)
            }
        }
    }
}

@Composable
fun IntegrationPolicyEditor(server: ChatToolServer, onChange: (IntegrationPolicy) -> Unit) {
    val colors = MyTheme.colors

    if (server.locked) {
        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.Top) {
            Icon(MyIcon.Info, colors.textMuted, Tokens.Size.IconSm)

            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                Label("Ask before every call: on")
                Label("Review results before the agent sees them: on")
                Label(ZulipReason, style = Tokens.Type.Meta, color = colors.textMuted)
            }
        }

        return
    }

    val policy = server.policy

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4)) {
        Label("A rule on a tool overrides its type, and a rule on a type overrides the integration default.", style = Tokens.Type.Meta, color = colors.textMuted)
        PolicySection("Ask before", "Ask before running", policy.ask, server, !server.alwaysAllow, builtIn(server)) { onChange(policy.copy(ask = it)) }
        PolicySection("Review", "Hold the result for your review for", policy.review, server, false, false) { onChange(policy.copy(review = it)) }
    }
}
