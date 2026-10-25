package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
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
import androidx.compose.ui.text.style.TextAlign
import kotlin.math.absoluteValue
import kotlin.math.roundToInt
import kotlin.math.roundToLong
import mycelium.api.ModelProviderUsage
import mycelium.api.OpenRouterSpend
import mycelium.state.AgentAccount
import mycelium.state.ago
import mycelium.state.compactWindowUsage
import mycelium.state.windowName

@Composable
fun UsageCard(
    accounts: List<Pair<AgentAccount, Boolean>>,
    modifier: Modifier = Modifier,
    providers: List<ModelProviderUsage> = emptyList(),
    currentProviderId: String? = null,
) {
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

            if (reported.isEmpty() && providers.isEmpty()) {
                Hairline()
                Label("No account has reported usage yet.", Modifier.padding(Tokens.Space.S1_5), Tokens.Type.Meta, colors.textMuted)
            }

            val accountBlocks: @Composable () -> Unit = {
                for ((account, thisChat) in reported) {
                    Hairline()

                    UsageBlock(account, thisChat)
                }
            }
            val providerBlocks: @Composable () -> Unit = {
                for (provider in providers.sortedBy { it.providerId != currentProviderId }) {
                    Hairline()

                    ProviderBlock(provider, provider.providerId == currentProviderId)
                }
            }

            if (providers.any { it.providerId == currentProviderId }) {
                providerBlocks()
                accountBlocks()
            } else {
                accountBlocks()
                providerBlocks()
            }
        }
    }
}

@Composable
private fun UsageHeader(title: String, thisChat: Boolean, readAt: String?, leading: @Composable RowScope.() -> Unit = {}) {
    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
        leading()
        Label(title, style = Tokens.Type.UiStrong, maxLines = 1)

        if (thisChat) {
            Tag("This chat")
        }

        Label(readAt?.let { "last updated ${ago(it)}" }.orEmpty(), Modifier.weight(1f), Tokens.Type.Meta.copy(textAlign = TextAlign.End), MyTheme.colors.textMuted, maxLines = 1)
    }
}

@Composable
private fun UsageLine(name: String, note: String?, value: String) {
    val colors = MyTheme.colors

    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
        Label(name, style = Tokens.Type.Meta, maxLines = 1)

        Label(note.orEmpty(), Modifier.weight(1f), Tokens.Type.Meta, colors.textMuted, maxLines = 1)
        Label(value, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
    }
}

@Composable
private fun UsageBlock(account: AgentAccount, thisChat: Boolean) {
    val platform = LocalPlatform.current
    val localDateTime: (Long) -> String? = { millis -> platform?.localDateTime(millis) }

    Column(Modifier.fillMaxWidth().padding(Tokens.Space.S1_5), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
        UsageHeader(account.label.substringAfter('@'), thisChat, account.usageReadAt) {
            Icon(agentIcon(account.agent), MyTheme.colors.text, Tokens.Size.IconMd)
        }

        val usages = account.windows.map { it to compactWindowUsage(it, localDateTime) }
        val weekExhausted = usages.any { (window, usage) -> window.name.startsWith("seven_day") && usage.percent >= 100 }

        for ((window, usage) in usages) {
            if (weekExhausted && window.name == "five_hour") continue

            val name = windowName(window.name).replaceFirstChar { it.uppercase() }

            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5)) {
                UsageLine(name, usage.forecast, usage.summary)
                Meter(usage.percent, "$name ${usage.percent}% used", attention = usage.attention)
            }
        }
    }
}

@Composable
private fun ProviderBlock(provider: ModelProviderUsage, thisChat: Boolean) {
    val title = if (provider.name.equals("OpenRouter", ignoreCase = true)) "OpenRouter" else "OpenRouter: ${provider.name}"

    Column(Modifier.fillMaxWidth().padding(Tokens.Space.S1_5), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
        UsageHeader(title, thisChat, provider.readAt)

        val spend = provider.spend

        if (spend == null) {
            Label("Spend not reported yet.", style = Tokens.Type.Meta, color = MyTheme.colors.textMuted)
        } else {
            SpendLines(spend)
        }
    }
}

@Composable
private fun SpendLines(spend: OpenRouterSpend) {
    val limit = spend.limit

    if (limit != null) {
        val (name, used) = when (spend.limitReset) {
            "daily" -> "Daily limit" to spend.daily
            "weekly" -> "Weekly limit" to spend.weekly
            "monthly" -> "Monthly limit" to spend.monthly
            else -> "Key limit" to spend.total
        }
        val percent = if (used == null || limit <= 0.0) 0 else (used / limit * 100).roundToInt().coerceIn(0, 100)

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5)) {
            UsageLine(name, null, "${usd(used)} / ${usd(limit)}")
            Meter(percent, "$name $percent% used")
        }
    } else {
        UsageLine("Today", null, usd(spend.daily))
    }

    if (limit == null && spend.limitRemaining != null) {
        UsageLine("Key limit remaining", null, usd(spend.limitRemaining))
    }

    UsageLine("Total", null, usd(spend.total))
}

private fun usd(amount: Double?): String {
    val cents = amount?.let { (it * 100).roundToLong() } ?: return "Not reported"
    val sign = if (cents < 0) "-" else ""
    val absolute = cents.absoluteValue

    return "$sign$${absolute / 100}.${(absolute % 100).toString().padStart(2, '0')}"
}
