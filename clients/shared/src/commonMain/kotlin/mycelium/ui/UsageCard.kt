package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.hoverable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import kotlin.time.Clock
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import mycelium.state.accountShortName
import mycelium.state.evenPacePercent
import mycelium.state.nextUsageTick
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import kotlin.time.Duration.Companion.minutes
import kotlin.time.Instant
import mycelium.state.SpendFigure
import mycelium.state.apiSpendRow
import mycelium.state.usageUpdated
import mycelium.state.windowShortName
import mycelium.api.ModelProviderUsage
import mycelium.api.ResetCredits
import androidx.compose.runtime.rememberCoroutineScope
import kotlinx.coroutines.launch
import kotlin.uuid.ExperimentalUuidApi
import kotlin.uuid.Uuid
import mycelium.api.UseResetRequest
import mycelium.state.AgentAccount
import mycelium.state.Store
import mycelium.state.UnnamedAccount
import mycelium.state.bankedResetsSummary
import mycelium.state.expiryDay
import mycelium.state.instantOrNull
import mycelium.state.compactWindowUsage
import mycelium.state.windowName

@Composable
fun UsageCard(
    accounts: List<Pair<AgentAccount, Boolean>>,
    modifier: Modifier = Modifier,
    providers: List<ModelProviderUsage> = emptyList(),
    currentProviderId: String? = null,
    onOpenHistory: (() -> Unit)? = null,
    onUseReset: (suspend (AgentAccount, String, String?) -> String)? = null,
) {
    val colors = MyTheme.colors
    var now by remember { mutableStateOf(Clock.System.now()) }

    LaunchedEffect(now) {
        delay(UpdatedTick)
        now = Clock.System.now()
    }

    BoxWithConstraints(modifier) {
        Column(
            Modifier
                .fillMaxWidth()
                .heightIn(max = maxHeight / 2)
                .background(colors.bgRaised, BlockShape)
                .border(Tokens.Size.Hairline, colors.border, BlockShape)
                .verticalScroll(rememberScrollState()),
        ) {
            val reported = accounts.filter { (account, _) -> account.windows.isNotEmpty() }
            val updated = usageUpdated(reported.mapNotNull { (account, _) -> account.usageReadAt } + providers.mapNotNull { it.readAt }, now)

            Row(Modifier.fillMaxWidth().padding(start = Tokens.Space.S1_5, top = Tokens.Space.S0_5, end = Tokens.Space.S1, bottom = Tokens.Space.S0_5), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
                Label("Usage", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
                Label(updated.orEmpty(), Modifier.weight(1f), Tokens.Type.Meta.copy(textAlign = TextAlign.End), colors.textMuted, maxLines = 1)

                if (onOpenHistory != null) {
                    MyButton(onOpenHistory, icon = MyIcon.Move, description = "Usage history", size = ButtonSize.Sm, muted = true)
                }
            }

            if (reported.isEmpty() && providers.isEmpty()) {
                Hairline()
                Label("No account has reported usage yet.", Modifier.padding(Tokens.Space.S1_5), Tokens.Type.Meta, colors.textMuted)
            }

            val accountBlocks: @Composable () -> Unit = {
                for ((account, thisChat) in reported) {
                    Hairline()

                    UsageBlock(account, thisChat, onUseReset)
                }
            }
            val providerBlocks: @Composable () -> Unit = {
                for (provider in providers.sortedBy { it.providerId != currentProviderId }) {
                    Hairline()

                    ProviderRow(provider, now)
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
private fun Hinted(hint: String, modifier: Modifier = Modifier, content: @Composable () -> Unit) {
    val source = remember { MutableInteractionSource() }

    Box(modifier.hoverable(source)) {
        content()
        Tooltip(hint, source)
    }
}

@Composable
internal fun UsageBlock(account: AgentAccount, thisChat: Boolean, onUseReset: (suspend (AgentAccount, String, String?) -> String)?) {
    val platform = LocalPlatform.current
    val colors = MyTheme.colors
    val localDateTime: (Long) -> String? = { millis -> platform?.localDateTime(millis) }
    var now by remember { mutableStateOf(Clock.System.now()) }

    LaunchedEffect(account.windows, now) {
        delay(nextUsageTick(account.windows, now))
        now = Clock.System.now()
    }

    Column(Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S1_5, vertical = Tokens.Space.S0_5)) {
        val usages = account.windows.map { it to compactWindowUsage(it, localDateTime, now) }
        val weekExhausted = usages.any { (window, usage) -> window.name.startsWith("seven_day") && usage.percent >= 100 }
        val shown = usages.filterNot { (window, _) -> weekExhausted && window.name == "five_hour" }

        for ((index, entry) in shown.withIndex()) {
            val (window, usage) = entry
            val name = windowName(window.name).replaceFirstChar { it.uppercase() }
            val pace = evenPacePercent(window, now)
            val description = listOfNotNull(name, "${usage.percent}% used", usage.resetDescription.takeIf { it.isNotEmpty() }, pace?.let { "even pace $it%" }).joinToString(", ")
            val attention = usage.attention

            Row(Modifier.fillMaxWidth().height(UsageRowHeight), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
                Row(Modifier.width(UsageNameWidth), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
                    if (index == 0) {
                        Icon(agentIcon(account.agent), colors.text, Tokens.Size.IconSm)
                        Label(accountShortName(account.label), style = Tokens.Type.MetaStrong, maxLines = 1)
                    } else if (thisChat && index == 1) {
                        Label("This chat", style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
                    }
                }

                Hinted(name, Modifier.widthIn(min = UsageWindowWidth)) {
                    Label(windowShortName(window.name), style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1, overflow = TextOverflow.Clip)
                }
                Hinted(description, Modifier.weight(1f).widthIn(min = UsageMeterMinWidth)) {
                    Meter(usage.percent, description, attention = attention, markerPercent = pace, height = UsageMeterHeight)
                }
                Label("${usage.percent}%", Modifier.width(UsagePercentWidth), Tokens.Type.Meta.copy(textAlign = TextAlign.End), maxLines = 1)
                Hinted(description, Modifier.width(UsageResetWidth)) {
                    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5, Alignment.End), verticalAlignment = Alignment.CenterVertically) {
                        if (attention) {
                            Icon(MyIcon.Alert, colors.statusAttention, UsageAlertSize)
                        }

                        Label(usage.reset, style = Tokens.Type.Meta, color = if (attention) colors.statusAttention else colors.textMuted, maxLines = 1)
                    }
                }
            }
        }

        if (thisChat && shown.size < 2) {
            Row(Modifier.height(UsageRowHeight), verticalAlignment = Alignment.CenterVertically) {
                Label("This chat", style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
            }
        }

        val banked = account.resetCredits?.takeIf { it.availableCount > 0 }

        if (banked != null) {
            BankedResets(account, banked, localDateTime, onUseReset)
        }
    }
}

@OptIn(ExperimentalUuidApi::class)
@Composable
private fun BankedResets(account: AgentAccount, banked: ResetCredits, localDateTime: (Long) -> String?, onUseReset: (suspend (AgentAccount, String, String?) -> String)?) {
    val scope = rememberCoroutineScope()
    var key by remember { mutableStateOf<String?>(null) }
    var failure by remember { mutableStateOf<String?>(null) }
    var outcome by remember { mutableStateOf<String?>(null) }
    var working by remember { mutableStateOf(false) }

    Row(Modifier.fillMaxWidth().height(Tokens.Size.ControlSm), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
        Hinted(bankedResetsSummary(banked, localDateTime), Modifier.weight(1f)) {
            Label(bankedResetsSummary(banked, localDateTime, brief = true), style = Tokens.Type.Meta, color = MyTheme.colors.textMuted, maxLines = 1)
        }

        if (onUseReset != null) {
            MyButton({ key = Uuid.random().toString() }, text = "Use reset", description = "Use a banked reset for ${account.label}", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
        }
    }

    outcome?.let { Label(it, style = Tokens.Type.Meta, color = MyTheme.colors.textMuted) }

    val attempt = key
    val recorded = banked.credits.orEmpty().filter { it.manual }.sortedBy { it.expiresAt }

    if (onUseReset != null && attempt != null) {
        var chosen by remember(attempt) { mutableStateOf(recorded.firstOrNull()?.id) }
        val chosenCredit = recorded.firstOrNull { it.id == chosen }
        val options = recorded.map { credit -> credit.id to "${credit.title}, expires ${credit.expiresAt?.let(::instantOrNull)?.let { expiryDay(it, localDateTime) }}" }

        Dialog(onDismiss = { key = null; failure = null }) {
            Label("Use a banked reset?", Modifier.semantics { heading() }, Tokens.Type.Title)

            if (recorded.size > 1) {
                Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                    Label("Which reset", style = Tokens.Type.MetaStrong, color = MyTheme.colors.textMuted)
                    Segmented(options.map { it.second }, options.first { it.first == chosen }.second, { label -> chosen = options.first { it.second == label }.first }, groupLabel = "Which reset")
                }
            }

            if (chosenCredit != null) {
                Label("This marks the ${chosenCredit.title} as used in Mycelium. Use it in Claude's settings too.", style = Tokens.Type.Meta, color = MyTheme.colors.textMuted)
            } else {
                Label("Codex resets the current usage limits for ${account.label}. You have ${banked.availableCount} banked. This cannot be undone.", style = Tokens.Type.Meta, color = MyTheme.colors.textMuted)
            }

            failure?.let { ErrorBanner(it, onDismiss = { failure = null }) }

            FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                MyButton({ key = null; failure = null }, text = "Cancel", muted = true)
                MyButton(
                    {
                        if (!working) {
                            working = true
                            failure = null
                            scope.launch {
                                try {
                                    outcome = resetOutcomeMessage(onUseReset(account, attempt, chosen))
                                    key = null
                                } catch (cancelled: CancellationException) {
                                    throw cancelled
                                } catch (problem: Exception) {
                                    failure = if (chosenCredit != null) "Mycelium could not mark the reset as used. Try again." else problem.message ?: "Codex could not use the reset. Try again."
                                }

                                working = false
                            }
                        }
                    },
                    text = if (chosenCredit != null) "Mark as used" else "Use reset",
                    variant = ButtonVariant.Primary,
                )
            }
        }
    }
}

internal suspend fun useBankedReset(store: Store, account: AgentAccount, key: String, creditId: String?): String =
    store.api.useReset(UseResetRequest(account.agent, account.label.takeUnless { it == UnnamedAccount }, key, creditId)).outcome

private fun resetOutcomeMessage(outcome: String): String = when (outcome) {
    "reset", "alreadyRedeemed" -> "Reset used."
    "nothingToReset" -> "Nothing to reset yet. Your reset is still banked."
    "marked" -> "Marked as used."
    "noCredit" -> "No banked resets are left."
    else -> "Codex answered $outcome."
}

@Composable
private fun ProviderRow(provider: ModelProviderUsage, now: Instant) {
    val colors = MyTheme.colors
    val row = apiSpendRow(provider, now)

    Row(Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S1_5, vertical = Tokens.Space.S0_5).height(UsageRowHeight), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
        Row(Modifier.width(UsageNameWidth), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
            Icon(MyIcon.Key, colors.text, Tokens.Size.IconSm)
            Label(row.name, style = Tokens.Type.MetaStrong, maxLines = 1)
        }

        Hinted(row.tooltip, Modifier.weight(1f).semantics { contentDescription = "${row.name}. ${row.tooltip}" }) {
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween, verticalAlignment = Alignment.CenterVertically) {
                if (row.problem != null) {
                    Label(row.problem, style = Tokens.Type.Meta, color = if (provider.status == "refused") colors.statusFailed else colors.textMuted, maxLines = 1)
                } else {
                    row.today?.let { SpendFigureLabel(it) }
                    row.month?.let { SpendFigureLabel(it) }
                }
            }
        }
    }
}

@Composable
private fun SpendFigureLabel(figure: SpendFigure) {
    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5)) {
        Label(figure.amount, style = Tokens.Type.Meta, maxLines = 1)
        Label(figure.label, style = Tokens.Type.Meta, color = MyTheme.colors.textMuted, maxLines = 1)
    }
}

private val UsageRowHeight = 18.dp
private val UsageNameWidth = 92.dp
private val UsageWindowWidth = 14.dp
private val UsageMeterMinWidth = 36.dp
private val UsagePercentWidth = 28.dp
private val UsageResetWidth = 52.dp
private val UsageMeterHeight = 4.dp
private val UsageAlertSize = 12.dp
private val UpdatedTick = 1.minutes
