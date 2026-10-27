package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import mycelium.api.Conversation
import mycelium.api.IntegrationCall
import mycelium.state.ColumnAlign
import mycelium.state.Store
import mycelium.state.agentName
import kotlin.time.Clock
import kotlin.time.Duration.Companion.days
import kotlin.time.Instant

private const val AnyIntegration = "All integrations"
private const val AnyWho = "Any agent or provider"
private const val AnyDecision = "Any decision"
private const val AnyRange = "All time"
val UsageRanges = listOf("Today" to 1, "7 days" to 7, "30 days" to 30, AnyRange to 0)
private val DecisionOptions = listOf(AnyDecision, "Approved", "Declined", "Allowed")

data class UsageRow(
    val conversationId: String?,
    val title: String,
    val where: String,
    val who: String,
    val tools: List<String>,
    val calls: Int,
    val approved: Int,
    val declined: Int,
    val sent: Int,
    val edited: Int,
    val notSent: Int,
    val lastAt: String,
)

data class ToolUsage(val tool: String, val calls: Int, val declined: Int, val notSent: Int?, val lastAt: String)

fun decisionWord(call: IntegrationCall): String = when (call.decision) {
    "declined" -> "Declined"
    "approved" -> "Approved"
    else -> "Allowed"
}

fun reviewWord(call: IntegrationCall): String = when (call.review) {
    "shared" -> "Sent as it was"
    "edited" -> "Edited"
    "not_sent" -> "Not sent"
    else -> if (call.kind == "write") "Not edited" else ""
}

fun surfaceWord(surface: String): String = when (surface) {
    "chat" -> "Chat"
    "assistant" -> "Assistant"
    else -> "Workspace conversation"
}

fun whoUsed(call: IntegrationCall, conversation: Conversation?): String = when {
    call.provider != null -> "${call.provider} · a chat model"
    conversation != null -> agentName(conversation.agent)
    else -> "An agent"
}

fun inRange(call: IntegrationCall, days: Int, now: Instant = Clock.System.now()): Boolean {
    if (days == 0) {
        return true
    }

    val at = runCatching { Instant.parse(call.at) }.getOrNull() ?: return true

    return at >= now - days.days
}

fun usageRows(calls: List<IntegrationCall>, conversations: Map<String, Conversation>): List<UsageRow> =
    calls.groupBy { it.conversationId ?: "none:${it.surface}:${it.provider}" }.map { (_, group) ->
        val first = group.first()
        val conversation = first.conversationId?.let { conversations[it] }

        UsageRow(
            first.conversationId,
            conversation?.title ?: if (first.conversationId == null) "Outside a conversation" else "A conversation",
            surfaceWord(first.surface),
            whoUsed(first, conversation),
            group.map { it.tool }.distinct().sorted(),
            group.size,
            group.count { it.decision != "declined" },
            group.count { it.decision == "declined" },
            group.count { it.review == "shared" },
            group.count { it.review == "edited" },
            group.count { it.review == "not_sent" },
            group.maxOf { it.at },
        )
    }.sortedByDescending { it.lastAt }

fun toolUsage(calls: List<IntegrationCall>): List<ToolUsage> =
    calls.groupBy { it.tool }.map { (tool, group) ->
        ToolUsage(tool, group.size, group.count { it.decision == "declined" }, if (group.first().kind == "write") null else group.count { it.review == "not_sent" }, group.maxOf { it.at })
    }.sortedByDescending { it.calls }

@Composable
private fun Stat(value: Int, words: String, modifier: Modifier = Modifier) {
    val colors = MyTheme.colors

    Column(modifier.padding(end = Tokens.Space.S4)) {
        Label(value.toString(), style = Tokens.Type.Title, maxLines = 1)
        Label(words, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
    }
}

@Composable
fun IntegrationUsage(store: Store, integration: Integration, onOpenActivity: () -> Unit, onOpenConversation: (String) -> Unit) {
    val colors = MyTheme.colors
    val live by store.state.collectAsState()
    var loaded by remember(integration.key) { mutableStateOf<List<IntegrationCall>?>(null) }
    var range by remember { mutableStateOf("30 days") }
    var account by remember { mutableStateOf("Every account") }
    val stamp = timestamper()
    val accountNames = integration.accounts.associate { it.id to accountTitle(it) }

    LaunchedEffect(integration.key, integration.accounts.size) {
        store.act { loaded = integration.accounts.flatMap { store.api.integrationCalls(server = it.id) }.sortedByDescending { it.at } }
    }

    val everything = loaded

    if (everything == null) {
        Label("Loading usage.", color = colors.textMuted)

        return
    }

    val days = UsageRanges.first { it.first == range }.second
    val calls = everything.filter { inRange(it, days) && (account == "Every account" || accountNames[it.serverId] == account) }
    val rows = usageRows(calls, live.conversations)
    val byTool = toolUsage(calls)

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4)) {
        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
            FilterMenu("Range", UsageRanges.map { it.first }, range, { range = it })

            if (integration.accounts.size > 1) {
                FilterMenu("Account", listOf("Every account") + accountNames.values, account, { account = it })
            }

            Spacer(Modifier.weight(1f))
            MyButton(onOpenActivity, text = "Open in activity log", trailingIcon = MyIcon.ChevronRight, variant = ButtonVariant.Outline, size = ButtonSize.Sm)
        }

        FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            Stat(calls.size, "calls")
            Stat(calls.count { it.decision == "declined" }, "declined by you")
            Stat(calls.count { it.review == "not_sent" }, "results not sent")
            Stat(calls.count { it.review == "edited" }, "results edited")
        }

        if (calls.isEmpty()) {
            Label("No calls in this range.", color = colors.textMuted)

            return@Column
        }

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            Label("Who used it", Modifier.semantics { heading() }, Tokens.Type.UiStrong)

            Table(
                listOf(
                    Col("Conversation", 120.dp, weight = 1f),
                    Col("Where", 96.dp, max = 180.dp, priority = 1),
                    Col("Agent or provider", 96.dp, max = 200.dp, priority = 2),
                    Col("Tools", 96.dp, max = 220.dp, priority = 3),
                    Col("Calls", 48.dp, align = ColumnAlign.End, numeric = true),
                    Col("Approved, declined", 96.dp, priority = 2),
                    Col("Sent, edited, not sent", 96.dp, priority = 3),
                    Col("Last used", 80.dp, priority = 1),
                ),
                rows.size,
                { row, column ->
                    val usage = rows[row]

                    when (column) {
                        0 -> TableCell.Text(usage.title)
                        1 -> TableCell.Text(usage.where, color = colors.textMuted)
                        2 -> TableCell.Text(usage.who)
                        3 -> TableCell.Text(usage.tools.joinToString(", "), Tokens.Type.Code, colors.textMuted)
                        4 -> TableCell.Text(usage.calls.toString())
                        5 -> TableCell.Text("${usage.approved}, ${usage.declined}")
                        6 -> TableCell.Text("${usage.sent}, ${usage.edited}, ${usage.notSent}")
                        else -> TableCell.Text(stamp(usage.lastAt, true), color = colors.textMuted)
                    }
                },
                rowKey = { rows[it].conversationId ?: "row-$it" },
                onRow = { index -> rows[index].conversationId?.let(onOpenConversation) },
                rowDescription = { "Open ${rows[it].title}" },
            )
        }

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            Label("By tool", Modifier.semantics { heading() }, Tokens.Type.UiStrong)

            Table(
                listOf(
                    Col("Tool", 120.dp, weight = 1f),
                    Col("Calls", 48.dp, align = ColumnAlign.End, numeric = true),
                    Col("Declined", 64.dp, align = ColumnAlign.End, numeric = true),
                    Col("Not sent", 96.dp, align = ColumnAlign.End),
                    Col("Last used", 80.dp, priority = 1),
                ),
                byTool.size,
                { row, column ->
                    val usage = byTool[row]

                    when (column) {
                        0 -> TableCell.Text(usage.tool, Tokens.Type.Code)
                        1 -> TableCell.Text(usage.calls.toString())
                        2 -> TableCell.Text(usage.declined.toString())
                        3 -> TableCell.Text(usage.notSent?.toString() ?: "not edited", color = colors.textMuted)
                        else -> TableCell.Text(stamp(usage.lastAt, true), color = colors.textMuted)
                    }
                },
                rowKey = { byTool[it].tool },
                rowDescription = { byTool[it].tool },
            )
        }
    }
}

@Composable
fun IntegrationsActivity(store: Store, integrations: List<Integration>, startOn: Integration?, onBack: () -> Unit, onOpenConversation: (String) -> Unit) {
    val colors = MyTheme.colors
    val live by store.state.collectAsState()
    var loaded by remember { mutableStateOf<List<IntegrationCall>?>(null) }
    var integration by remember { mutableStateOf(startOn?.name ?: AnyIntegration) }
    var conversation by remember { mutableStateOf("") }
    var who by remember { mutableStateOf(AnyWho) }
    var range by remember { mutableStateOf("30 days") }
    var decision by remember { mutableStateOf(AnyDecision) }
    val stamp = timestamper()

    LaunchedEffect(Unit) {
        store.act { loaded = store.api.integrationCalls() }
    }

    val everything = loaded

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
        BackButton("Integrations", onBack, showText = true)
        Label("Activity", Modifier.semantics { heading() }, Tokens.Type.UiStrong)

        if (everything == null) {
            Label("Loading activity.", color = colors.textMuted)

            return@Column
        }

        val conversations = live.conversations
        val whoOptions = (everything.map { whoUsed(it, it.conversationId?.let { id -> conversations[id] }) }).distinct().sorted()
        val serverIds = integrations.find { it.name == integration }?.accounts?.map { it.id }
        val days = UsageRanges.first { it.first == range }.second
        val shown = everything.filter { call ->
            val title = call.conversationId?.let { conversations[it]?.title }.orEmpty()

            (serverIds == null || call.serverId in serverIds) &&
                (conversation.isBlank() || conversation.trim().lowercase() in title.lowercase()) &&
                (who == AnyWho || whoUsed(call, call.conversationId?.let { conversations[it] }) == who) &&
                inRange(call, days) &&
                (decision == AnyDecision || decisionWord(call) == decision)
        }

        FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            FilterMenu("Integration", listOf(AnyIntegration) + integrations.map { it.name }, integration, { integration = it })
            FilterMenu("Agent or provider", listOf(AnyWho) + whoOptions, who, { who = it })
            FilterMenu("Range", UsageRanges.map { it.first }, range, { range = it })
            FilterMenu("Decision", DecisionOptions, decision, { decision = it })
        }

        TextInput(conversation, { conversation = it }, label = "Filter by conversation", placeholder = "Filter by conversation", leadingIcon = MyIcon.Search, showLabel = false)

        if (shown.isEmpty()) {
            Label("No calls match. Widen the range or clear a filter.", color = colors.textMuted)

            return@Column
        }

        Table(
            listOf(
                Col("When", 80.dp),
                Col("Integration", 80.dp, max = 160.dp, priority = 1),
                Col("Conversation", 120.dp, weight = 1f),
                Col("Agent or provider", 96.dp, max = 200.dp, priority = 2),
                Col("Tool", 96.dp, max = 200.dp, priority = 1),
                Col("Decision", 72.dp),
                Col("Result", 96.dp, priority = 2),
            ),
            shown.size,
            { row, column ->
                val call = shown[row]
                val chat = call.conversationId?.let { conversations[it] }

                when (column) {
                    0 -> TableCell.Text(stamp(call.at, true), color = colors.textMuted)
                    1 -> TableCell.Text(call.serverName)
                    2 -> TableCell.Text(chat?.title ?: if (call.conversationId == null) "Outside a conversation" else "A conversation")
                    3 -> TableCell.Text(whoUsed(call, chat))
                    4 -> TableCell.Text(call.tool, Tokens.Type.Code)
                    5 -> TableCell.Text(decisionWord(call) + if (call.isError) ", failed" else "")
                    else -> TableCell.Text(reviewWord(call), color = colors.textMuted)
                }
            },
            rowKey = { shown[it].id },
            onRow = { index -> shown[index].conversationId?.let(onOpenConversation) },
            rowDescription = { "${shown[it].tool} on ${shown[it].serverName}" },
        )

        if (everything.size >= 100 && everything.size % 100 == 0) {
            Row {
                MyButton({
                    store.act { loaded = everything + store.api.integrationCalls(before = everything.last().at) }
                }, text = "Show older calls", variant = ButtonVariant.Outline)
            }
        }
    }
}
