package mycelium.state

import mycelium.api.Conversation
import mycelium.api.Event
import mycelium.api.EventKinds
import mycelium.api.LoginInfo
import mycelium.api.LoginState
import mycelium.api.ModelProvider
import mycelium.api.NodeSummary
import mycelium.api.ResetCredits
import mycelium.api.UsageWindow
import kotlinx.serialization.json.JsonObject
import kotlin.math.roundToInt
import kotlin.random.Random
import kotlin.time.Clock
import kotlin.time.Duration
import kotlin.time.Duration.Companion.days
import kotlin.time.Duration.Companion.hours
import kotlin.time.Duration.Companion.minutes
import kotlin.time.Duration.Companion.seconds
import kotlin.time.Instant

const val UnnamedAccount = "Account name not reported"

data class AccountOnNode(val nodeName: String, val state: LoginState)

data class AgentAccount(val agent: String, val label: String, val method: String, val nodes: List<AccountOnNode>, val windows: List<UsageWindow> = emptyList(), val usageReadAt: String? = null, val resetCredits: ResetCredits? = null)

fun agentAccounts(nodes: Collection<NodeSummary>): List<AgentAccount> {
    val grouped = linkedMapOf<Triple<String, String?, String?>, MutableList<Pair<NodeSummary, LoginInfo>>>()

    for (node in nodes.sortedBy { it.name }) {
        for (login in node.logins) {
            val account = login.account?.trim()?.ifBlank { null }
            val unused = login.state == LoginState.SignedOut && login.windows.isEmpty() && login.usageReadAt == null && login.usageLeft == null

            if (unused && account?.matches(Regex("account-[0-9]{4}")) == true) {
                continue
            }

            val key = Triple(login.agent, account?.lowercase(), if (account == null) node.id else null)
            grouped.getOrPut(key) { mutableListOf() }.add(node to login)
        }
    }

    return grouped.map { (key, entries) ->
        val freshest = entries.map { it.second }.filter { it.windows.isNotEmpty() }.maxByOrNull { it.usageReadAt.orEmpty() }

        AgentAccount(key.first, entries.first().second.account?.trim()?.ifBlank { null } ?: UnnamedAccount, entries.first().second.method, entries.map { (node, login) -> AccountOnNode(node.name, login.state) }, freshest?.windows.orEmpty(), freshest?.usageReadAt, freshest?.resetCredits)
    }.sortedWith(compareBy({ AgentOrder.indexOf(it.agent).let { index -> if (index < 0) AgentOrder.size else index } }, { it.label }))
}

data class AgentAccountSplit(val accounts: List<AgentAccount>, val signedOutNodes: List<String>)

fun splitSignedOut(accounts: List<AgentAccount>): AgentAccountSplit {
    val (signedOut, real) = accounts.partition { it.label == UnnamedAccount && it.nodes.all { node -> node.state == LoginState.SignedOut } }

    return AgentAccountSplit(real, signedOut.flatMap { it.nodes.map { node -> node.nodeName } }.distinct().sorted())
}

fun chatAccountFirst(accounts: List<AgentAccount>, agent: String?, nodeName: String?, account: String?): List<Pair<AgentAccount, Boolean>> {
    val current = accounts.firstOrNull { candidate ->
        candidate.agent == agent && candidate.nodes.any { it.nodeName == nodeName } && (account == null || candidate.label == account)
    }

    return accounts.sortedBy { if (it == current) 0 else 1 }.map { it to (it == current) }
}

fun bankedResetsSummary(credits: ResetCredits, localDateTime: (Long) -> String?): String {
    val count = if (credits.availableCount == 1) "1 reset banked" else "${credits.availableCount} resets banked"
    val next = credits.credits.orEmpty().mapNotNull { credit -> credit.expiresAt?.let { instantOrNull(it) } }.minOrNull() ?: return count
    val date = (localDateTime(next.toEpochMilliseconds()) ?: next.toString()).substringBefore('T')
    val (_, month, day) = date.split('-')

    return "$count, next expires ${day.toInt()} ${MonthNames[month.toInt() - 1]}"
}

fun runsOutBeforeReset(window: UsageWindow): Boolean {
    val runsOut = window.runsOutAt?.let { instantOrNull(it) } ?: return false
    val resets = window.resetsAt?.let { instantOrNull(it) } ?: return false

    return runsOut < resets
}

fun accountsOn(node: NodeSummary, agent: String): List<LoginInfo> = node.logins.filter { it.agent == agent }

fun switchableAccounts(node: NodeSummary?, agent: String): List<LoginInfo> = node?.let { accountsOn(it, agent) }.orEmpty().filter { it.state == LoginState.SignedIn }.takeIf { it.size >= 2 }.orEmpty()

const val DefaultAccountId = "(default)"

const val SignInPrefix = "sign in:"

fun accountChoiceId(login: LoginInfo): String = login.account ?: DefaultAccountId

fun accountsElsewhere(nodes: Collection<NodeSummary>, node: NodeSummary, agent: String): List<String> {
    if (node.agents.none { it.agent == agent && it.available }) {
        return emptyList()
    }

    val here = accountsOn(node, agent).filter { it.state == LoginState.SignedIn }.mapNotNull { it.account }.toSet()

    return nodes.filter { it.id != node.id }.flatMap { accountsOn(it, agent) }.filter { it.state == LoginState.SignedIn }.mapNotNull { it.account }.distinct().filter { it !in here }.sorted()
}

fun accountChoices(logins: List<LoginInfo>, elsewhere: List<String>, nodeName: String): List<Choice> =
    logins.filter { it.state == LoginState.SignedIn }.map { login ->
        val nearest = login.windows.maxByOrNull { it.usedPercent }
        val left = nearest?.let { 100 - it.usedPercent.roundToInt().coerceIn(0, 100) } ?: login.usageLeft

        Choice(accountChoiceId(login), login.account ?: "Default account", when (login.state) {
            LoginState.SignedIn -> if (left == null) "Usage unknown" else if (nearest == null) "$left% left" else "$left% left, ${windowName(nearest.name)}"
            LoginState.Expired -> "Expired"
            LoginState.SignedOut -> "Signed out"
            LoginState.Unknown -> "Unknown"
        }, usedPercent = left?.let { 100 - it }.takeIf { login.state == LoginState.SignedIn })
    } + elsewhere.map { Choice(SignInPrefix + it, it, "Sign in on $nodeName") }

const val ContinueAccountPrefix = "account:"

const val ContinueAgentPrefix = "agent:"

const val ContinueChatPrefix = "chat:"

const val ContinueNodePrefix = "on:"

private const val LowUsageLeft = 10

private const val ChatGroup = "Chat"

private fun lowUsageHint(login: LoginInfo): String? = login.usageLeft?.takeIf { it < LowUsageLeft }?.let { "$it% left" }

fun currentContinueId(conversation: Conversation): String? = when {
    conversation.agent == "chat" -> conversation.providerId?.let { provider -> conversation.model?.let { "$ContinueChatPrefix$provider:$it" } }
    else -> "$ContinueAccountPrefix${conversation.account ?: DefaultAccountId}"
}

fun continueChoices(conversation: Conversation, nodes: Collection<NodeSummary>, providers: List<ModelProvider>): List<Choice> {
    val reachable = nodes.filter { it.online }.sortedBy { if (it.id == conversation.nodeId) 0 else 1 }
    val accounts = reachable.flatMap { node ->
        node.logins.filter { login -> login.state == LoginState.SignedIn && node.agents.any { it.agent == login.agent && it.available } }.map { node to it }
    }
    val harnesses = (listOf("claude", "codex") + accounts.map { it.second.agent }).distinct()
    val perHarness = harnesses.flatMap { agent ->
        accounts.filter { it.second.agent == agent }.map { (node, login) ->
            val here = node.id == conversation.nodeId
            val id = when {
                here && agent == conversation.agent -> "$ContinueAccountPrefix${accountChoiceId(login)}"
                here -> "$ContinueAgentPrefix$agent:${accountChoiceId(login)}"
                else -> "$ContinueNodePrefix${node.id}:$agent:${accountChoiceId(login)}"
            }
            val hint = listOfNotNull(lowUsageHint(login), if (here) null else "on ${node.name}").joinToString(", ").ifEmpty { null }

            Choice(id, "${agentName(agent)} · ${login.account ?: "Default account"}", hint, group = agentName(agent))
        }
    }
    val models = providers.mapNotNull { provider -> provider.defaultModel?.let { Choice("$ContinueChatPrefix${provider.id}:$it", "${provider.name} · $it", group = ChatGroup) } }

    return perHarness + models
}

fun windowName(name: String): String = when (name) {
    "five_hour" -> "5-hour window"
    "seven_day" -> "weekly window"
    else -> name.removePrefix("seven_day_").replace('_', ' ').let { "weekly $it window" }
}

fun usageClockTime(iso: String, localDateTime: (Long) -> String?, now: Instant = Clock.System.now()): String {
    val moment = instantOrNull(iso) ?: return ""
    val local = localDateTime(moment.toEpochMilliseconds())
    val today = localDateTime(now.toEpochMilliseconds())

    if (local == null || today == null) {
        return until(iso, now)
    }

    val time = local.substringAfter('T')

    val date = local.substringBefore('T')

    if (date == today.substringBefore('T')) {
        return time
    }

    val (_, month, day) = date.split('-')

    return "${day.toInt()} ${MonthNames[month.toInt() - 1]} $time"
}

private val MonthNames = listOf("Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec")

fun usageProjection(window: UsageWindow, localDateTime: (Long) -> String?, now: Instant = Clock.System.now()): String {
    val resets = window.resetsAt?.let { "resets ${usageClockTime(it, localDateTime, now)}" }
    val about = if (localDateTime(now.toEpochMilliseconds()) == null) "" else "about "
    val runsOut = window.runsOutAt?.let { "at this rate, runs out $about${usageClockTime(it, localDateTime, now)}" }
    val steady = if (window.runsOutAt == null && window.ratePerHour != null) "at this rate, lasts until it resets" else null

    return listOfNotNull(runsOut ?: steady, resets).joinToString(", ").replaceFirstChar { it.uppercase() }
}

data class CompactWindowUsage(val percent: Int, val forecast: String?, val summary: String, val attention: Boolean)

fun compactWindowUsage(window: UsageWindow, localDateTime: (Long) -> String?, now: Instant = Clock.System.now()): CompactWindowUsage {
    val resetsAt = window.resetsAt
    val resets = resetsAt?.let { instantOrNull(it) }

    if (resetsAt != null && resets != null && resets <= now) {
        return CompactWindowUsage(0, null, "0% used, reset ${usageClockTime(resetsAt, localDateTime, now)}", false)
    }

    val percent = window.usedPercent.roundToInt().coerceIn(0, 100)
    val attention = percent < 100 && runsOutBeforeReset(window)
    val forecast = window.runsOutAt?.takeIf { attention }?.let { "runs out ${until(it, now)}" }
    val left = resetsAt?.let { ", ${until(it, now).removePrefix("in ")} left" }.orEmpty()

    return CompactWindowUsage(percent, forecast, "$percent% used$left", attention)
}

private val FiveHours = 5.hours
private val SevenDays = 7.days
private val UsageTickCeiling = 1.minutes
private val UsageTickFloor = 1.seconds

fun evenPacePercent(window: UsageWindow, now: Instant = Clock.System.now()): Int? {
    val length = when {
        window.name == "five_hour" -> FiveHours
        window.name.startsWith("seven_day") -> SevenDays
        else -> return null
    }
    val resets = window.resetsAt?.let { instantOrNull(it) } ?: return null
    val remaining = resets - now

    if (remaining <= Duration.ZERO || remaining > length) {
        return null
    }

    return ((length - remaining) / length * 100).roundToInt().coerceIn(0, 100)
}

fun nextUsageTick(windows: List<UsageWindow>, now: Instant = Clock.System.now()): Duration {
    val untilReset = windows
        .mapNotNull { window -> window.resetsAt?.let { instantOrNull(it) } }
        .map { it - now }
        .filter { it > Duration.ZERO }
        .minOrNull() ?: UsageTickCeiling

    return minOf(untilReset, UsageTickCeiling).coerceAtLeast(UsageTickFloor)
}

fun accountMenuTitle(agent: String): String = when (agent) {
    "claude" -> "Claude account"
    "codex" -> "Codex account"
    else -> "Account"
}

fun newAccountName(): String = "account-${Random.nextInt(1000, 10000)}"

private val AgentOrder = listOf("claude", "codex")
