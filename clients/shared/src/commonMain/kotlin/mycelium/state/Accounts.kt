package mycelium.state

import mycelium.api.LoginInfo
import mycelium.api.LoginState
import mycelium.api.NodeSummary
import mycelium.api.UsageWindow
import kotlin.math.roundToInt
import kotlin.random.Random
import kotlin.time.Clock
import kotlin.time.Instant

const val UnnamedAccount = "Account name not reported"

data class AccountOnNode(val nodeName: String, val state: LoginState)

data class AgentAccount(val agent: String, val label: String, val method: String, val nodes: List<AccountOnNode>, val windows: List<UsageWindow> = emptyList(), val usageReadAt: String? = null)

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

        AgentAccount(key.first, entries.first().second.account?.trim()?.ifBlank { null } ?: UnnamedAccount, entries.first().second.method, entries.map { (node, login) -> AccountOnNode(node.name, login.state) }, freshest?.windows.orEmpty(), freshest?.usageReadAt)
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

fun runsOutBeforeReset(window: UsageWindow): Boolean {
    val runsOut = window.runsOutAt?.let { runCatching { Instant.parse(it) }.getOrNull() } ?: return false
    val resets = window.resetsAt?.let { runCatching { Instant.parse(it) }.getOrNull() } ?: return false

    return runsOut < resets
}

fun accountsOn(node: NodeSummary, agent: String): List<LoginInfo> = node.logins.filter { it.agent == agent }

fun switchableAccounts(node: NodeSummary?, agent: String): List<LoginInfo> = node?.let { accountsOn(it, agent) }.orEmpty().takeIf { it.size >= 2 }.orEmpty()

const val DefaultAccountId = "(default)"

const val SignInPrefix = "sign in:"

fun accountChoiceId(login: LoginInfo): String = login.account ?: DefaultAccountId

fun accountsElsewhere(nodes: Collection<NodeSummary>, node: NodeSummary, agent: String): List<String> {
    val here = accountsOn(node, agent).mapNotNull { it.account }.toSet()

    return nodes.filter { it.id != node.id }.flatMap { accountsOn(it, agent) }.mapNotNull { it.account }.distinct().filter { it !in here }.sorted()
}

fun accountChoices(logins: List<LoginInfo>, elsewhere: List<String>, nodeName: String): List<Choice> =
    logins.map { login ->
        val nearest = login.windows.maxByOrNull { it.usedPercent }
        val left = nearest?.let { 100 - it.usedPercent.roundToInt().coerceIn(0, 100) } ?: login.usageLeft

        Choice(accountChoiceId(login), login.account ?: "Default account", when (login.state) {
            LoginState.SignedIn -> if (left == null) "Usage unknown" else if (nearest == null) "$left% left" else "$left% left, ${windowName(nearest.name)}"
            LoginState.Expired -> "Expired"
            LoginState.SignedOut -> "Signed out"
            LoginState.Unknown -> "Unknown"
        }, usedPercent = left?.let { 100 - it }.takeIf { login.state == LoginState.SignedIn })
    } + elsewhere.map { Choice(SignInPrefix + it, it, "Sign in on $nodeName") }

fun windowName(name: String): String = when (name) {
    "five_hour" -> "5-hour window"
    "seven_day" -> "weekly window"
    else -> name.removePrefix("seven_day_").replace('_', ' ').let { "weekly $it window" }
}

fun usageClockTime(iso: String, localDateTime: (Long) -> String?, now: Instant = Clock.System.now()): String {
    val moment = runCatching { Instant.parse(iso) }.getOrNull() ?: return ""
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
    val resets = resetsAt?.let { runCatching { Instant.parse(it) }.getOrNull() }

    if (resetsAt != null && resets != null && resets <= now) {
        return CompactWindowUsage(0, null, "0% used, reset ${usageClockTime(resetsAt, localDateTime, now)}", false)
    }

    val percent = window.usedPercent.roundToInt().coerceIn(0, 100)
    val attention = percent < 100 && runsOutBeforeReset(window)
    val approximately = if (localDateTime(now.toEpochMilliseconds()) == null) "" else "~"
    val forecast = window.runsOutAt?.takeIf { attention }?.let { "runs out $approximately${usageClockTime(it, localDateTime, now)}" }
    val left = resetsAt?.let { ", ${until(it, now).removePrefix("in ")} left" }.orEmpty()

    return CompactWindowUsage(percent, forecast, "$percent% used$left", attention)
}

fun accountMenuTitle(agent: String): String = when (agent) {
    "claude" -> "Claude account"
    "codex" -> "Codex account"
    else -> "Account"
}

fun newAccountName(): String = "account-${Random.nextInt(1000, 10000)}"

private val AgentOrder = listOf("claude", "codex")
