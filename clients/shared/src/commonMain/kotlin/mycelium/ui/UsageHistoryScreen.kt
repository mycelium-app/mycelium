package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import kotlin.time.Instant
import mycelium.api.AccountPrice
import mycelium.api.UsageHistory
import mycelium.api.UsageHistoryAccount
import mycelium.api.UsageReading
import mycelium.state.ActivityRanges
import mycelium.state.Store
import mycelium.state.agentName
import mycelium.state.approximateDollars
import mycelium.state.approximateTokens
import mycelium.state.dateTimeLabel
import mycelium.state.dayLabel
import mycelium.state.formatMoney
import mycelium.state.yourPriceCaption
import mycelium.state.instantOrNull
import mycelium.state.planName
import mycelium.state.priceRatio
import mycelium.state.readingsNote
import mycelium.state.usageEstimateNote
import mycelium.state.sampleCount
import mycelium.state.usageAccountName

private val WindowColumns = listOf(TableColumn("Window started", 1.4f), TableColumn("Used", 0.7f, numeric = true), TableColumn("Tokens", 0.9f, numeric = true), TableColumn("Tokens per 1%", 1f, numeric = true), TableColumn("API equivalent", 1.1f, numeric = true))

private const val WindowRowLimit = 50
@Composable
@OptIn(ExperimentalLayoutApi::class)
fun UsageHistoryScreen(store: Store) {
    val colors = MyTheme.colors
    val platform = LocalPlatform.current
    var range by remember { mutableStateOf(ActivityRanges[1].second) }
    var chosen by remember { mutableStateOf<String?>(null) }
    var menu by remember { mutableStateOf(false) }
    var everyAccount by remember { mutableStateOf<UsageHistory?>(null) }
    var history by remember { mutableStateOf<UsageHistory?>(null) }
    var pricing by remember { mutableStateOf<UsageHistoryAccount?>(null) }
    var revision by remember { mutableStateOf(0) }

    LaunchedEffect(range, chosen, revision) {
        store.act {
            val loaded = store.api.usageHistory(range, chosen)

            history = loaded

            if (chosen == null) {
                everyAccount = loaded
            }
        }
    }

    Column(Modifier.fillMaxSize()) {
        Bar {
            OpenListButton()
            BarTitle("Usage history")
            Box(Modifier.weight(1f))
            Segmented(ActivityRanges.map { it.first }, ActivityRanges.first { it.second == range }.first, { label -> range = ActivityRanges.first { it.first == label }.second }, groupLabel = "Period")

            Box {
                val choices = everyAccount?.accounts.orEmpty()
                val selected = choices.firstOrNull { "${it.agent}:${it.account}" == chosen }

                MyButton({ menu = true }, text = selected?.let(::usageAccountName) ?: "All accounts", trailingIcon = MyIcon.ChevronDown, variant = ButtonVariant.Outline, description = "Choose accounts")

                if (menu) {
                    MenuPopup(onDismiss = { menu = false }) {
                        MenuItem("All accounts", onClick = { menu = false; chosen = null }, checked = chosen == null)

                        for (account in choices) {
                            MenuItem(usageAccountName(account), onClick = { menu = false; chosen = "${account.agent}:${account.account}" }, checked = chosen == "${account.agent}:${account.account}", leading = agentIcon(account.agent))
                        }
                    }
                }
            }
        }

        val loaded = history

        if (loaded == null) {
            Label("Loading usage history.", Modifier.padding(Tokens.Space.S4), color = colors.textMuted)

            return@Column
        }

        Column(Modifier.weight(1f).fillMaxWidth().verticalScrollWithBar().padding(Tokens.Space.S4), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4)) {
            if (loaded.accounts.isEmpty()) {
                Label("No account has reported usage yet.", color = colors.textMuted)

                return@Column
            }

            for (account in loaded.accounts) {
                AccountHistory(account, platform?.let { it::localDateTime }, onSetPrice = { pricing = it })
            }

            Label(usageEstimateNote(loaded.accounts), style = Tokens.Type.Meta, color = colors.textMuted)
        }
    }

    pricing?.let { account ->
        PriceDialog(
            store,
            account.agent,
            account.account,
            account.yourPrice?.let { AccountPrice(account.agent, account.account, it.amount, it.currency, it.period) },
            listPrice = account.planPriceUsd?.let { "${planName(account.plan.orEmpty())} list price of $${it.toInt()} a month" },
            onSaved = { revision += 1 },
            onDismiss = { pricing = null },
        )
    }
}

@Composable
@OptIn(ExperimentalLayoutApi::class)
private fun AccountHistory(account: UsageHistoryAccount, localDateTime: ((Long) -> String?)?, onSetPrice: (UsageHistoryAccount) -> Unit) {
    val colors = MyTheme.colors
    val name = usageAccountName(account)
    val agent = agentName(account.agent)
    val local: (Long) -> String? = { millis -> localDateTime?.invoke(millis) }

    Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
            Icon(agentIcon(account.agent), colors.text, Tokens.Size.IconMd)
            Label(name, Modifier.semantics { heading() }, Tokens.Type.UiStrong, maxLines = 1)
            account.plan?.let { Tag(planName(it)) }
            Box(Modifier.weight(1f))
            readingsNote(account)?.let { Label(it, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1) }
        }

        FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
            val perPoint = account.tokensPerPoint

            Kpi("Tokens per 1% of the 5-hour window", perPoint.fiveHour?.let(::approximateTokens) ?: "Not enough readings", kpiSlot(), caption = if (perPoint.fiveHour != null) sampleCount(perPoint.fiveHourSamples, "window", "windows") else "No 5-hour window has risen 5 points yet")
            Kpi("Tokens per 1% of the weekly window", perPoint.sevenDay?.let(::approximateTokens) ?: "Not enough readings", kpiSlot(), caption = if (perPoint.sevenDay != null) sampleCount(perPoint.sevenDaySamples, "week", "weeks") else "No weekly window has risen 5 points yet")
            Kpi("API equivalent", account.apiEquivalentMonthUsd?.let { "${approximateDollars(it)} this month" } ?: "Not reported", kpiSlot(), caption = account.apiEquivalentPerWindowUsd?.let { "${approximateDollars(it)} per 5-hour window" })
            val mine = account.yourPrice

            Kpi(
                if (mine != null) "Your price" else "Subscription",
                mine?.let { "${formatMoney(it.monthlyAmount, it.currency)} a month" } ?: account.planPriceUsd?.let { "$${it.toInt()} a month" } ?: "Plan price unknown",
                kpiSlot(),
                caption = if (mine != null) yourPriceCaption(account) else priceRatio(account) ?: if (account.plan == null) "$agent did not report a plan" else "No published price for ${planName(account.plan)}",
            )
        }

        MyButton({ onSetPrice(account) }, text = if (account.yourPrice == null) "Set your price" else "Change your price", variant = ButtonVariant.Outline, size = ButtonSize.Sm)

        val readingTimes = account.readings.mapNotNull { instantOrNull(it.at)?.toEpochMilliseconds() }
        val from = readingTimes.minOrNull() ?: 0L
        val until = readingTimes.maxOrNull() ?: 0L

        val fromLabel = dayLabel(Instant.fromEpochMilliseconds(from).toString().take(10))
        val untilLabel = dayLabel(Instant.fromEpochMilliseconds(until).toString().take(10))

        FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
            for ((window, title) in listOf("five_hour" to "5-hour window, % used", "seven_day" to "Weekly window, % used")) {
                val readings: List<UsageReading> = account.readings.filter { it.window == window }
                val peak = readings.maxByOrNull { it.percent }

                Column(kpiSlot().border(Tokens.Size.Hairline, colors.border, BlockShape).background(colors.bgPanel, BlockShape).padding(Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                    Label(title, Modifier.semantics { heading() }, Tokens.Type.Meta, colors.textMuted)

                    if (readings.isEmpty()) {
                        Label("No readings in this period.", style = Tokens.Type.Meta, color = colors.textMuted)
                    } else {
                        PercentLineChart(readings.mapNotNull { reading -> instantOrNull(reading.at)?.let { it.toEpochMilliseconds() to reading.percent } }, from, until, "$title for $name. Peak ${peak?.percent?.toInt()}% on ${peak?.let { dateTimeLabel(it.at, local) }}, ${readings.size} readings.", fromLabel, untilLabel)
                    }
                }
            }
        }

        val rows = account.windows.filter { it.window == "five_hour" }.sortedByDescending { it.startedAt }.take(WindowRowLimit)

        DataTable(
            "5-hour windows",
            WindowColumns,
            rows.map { window ->
                listOf(
                    dateTimeLabel(window.startedAt, local),
                    "${window.peakPercent.toInt()}%",
                    formatTokensOrDash(window.tokens),
                    window.tokensPerPoint?.let(::approximateTokens) ?: "Not enough",
                    window.apiEquivalentUsd?.let(::approximateDollars) ?: "Not reported",
                )
            },
            "No 5-hour windows in this period.",
        )
    }
}

private fun formatTokensOrDash(tokens: Long): String = if (tokens == 0L) "None recorded" else mycelium.state.formatTokens(tokens)
