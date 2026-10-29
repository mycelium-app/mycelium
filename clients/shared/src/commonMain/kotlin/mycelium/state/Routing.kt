package mycelium.state

import kotlin.math.roundToInt
import kotlin.time.Clock
import kotlin.time.Instant
import mycelium.api.AccountPrice
import mycelium.api.Conversation
import mycelium.api.LoginInfo
import mycelium.api.LoginState
import mycelium.api.ModelChoice
import mycelium.api.ModelProvider
import mycelium.api.NodeSummary
import mycelium.api.Routing
import mycelium.api.UsageWindow

const val RoutingPrefix = "routing:"

private const val FiveHour = "five_hour"

private const val SevenDay = "seven_day"

private const val Full = 100.0

data class Standing(val limited: Boolean, val fiveLeft: Double?, val weekLeft: Double?)

private fun leftIn(windows: List<UsageWindow>, name: String, now: Instant): Double? {
    val window = windows.firstOrNull { it.name == name } ?: return null
    val resets = window.resetsAt?.let { instantOrNull(it) }

    return if (resets != null && resets <= now) Full else (Full - window.usedPercent).coerceIn(0.0, Full)
}

fun loginStanding(login: LoginInfo, now: Instant = Clock.System.now()): Standing {
    if (login.windows.isEmpty()) {
        val left = login.usageLeft?.toDouble()

        return Standing(left != null && left <= 0.0, left, null)
    }

    val five = leftIn(login.windows, FiveHour, now)
    val week = leftIn(login.windows, SevenDay, now)

    return Standing(listOfNotNull(five, week).any { it <= 0.0 }, five, week)
}

fun bestLogin(logins: List<LoginInfo>, now: Instant = Clock.System.now()): LoginInfo? =
    logins
        .filter { it.state == LoginState.SignedIn && it.account != null }
        .map { it to loginStanding(it, now) }
        .filter { !it.second.limited }
        .sortedWith(
            compareByDescending<Pair<LoginInfo, Standing>> { it.second.fiveLeft != null }
                .thenByDescending { it.second.fiveLeft ?: -1.0 }
                .thenByDescending { it.second.weekLeft ?: -1.0 }
                .thenBy { it.first.account },
        )
        .firstOrNull()?.first

fun routingName(routing: String): String = when (routing) {
    Routing.ClaudeAny -> "Claude Code, any account"
    Routing.CodexAny -> "Codex, any account"
    else -> "Any model"
}

fun modelChoiceName(choice: ModelChoice, providers: List<ModelProvider>): String {
    val agent = choice.agent

    if (choice.anyModel) {
        return routingName(Routing.AnyModel)
    }

    if (agent != null) {
        val where = choice.account ?: "any account"

        return listOfNotNull("${agentName(agent)}, $where", choice.model).joinToString(" · ")
    }

    val provider = providers.firstOrNull { it.id == choice.providerId }?.name ?: "Chat"

    return listOfNotNull(provider, choice.model).joinToString(" · ")
}

fun anyAccountId(agent: String): String = RoutingPrefix + if (agent == "codex") Routing.CodexAny else Routing.ClaudeAny

fun routingButtonText(id: String?): String? = when (id?.removePrefix(RoutingPrefix)?.takeIf { id.startsWith(RoutingPrefix) }) {
    Routing.ClaudeAny, Routing.CodexAny -> "Any account"
    Routing.AnyModel -> "Any model"
    else -> null
}

fun routingAgent(id: String): String? = when (id.removePrefix(RoutingPrefix).takeIf { id.startsWith(RoutingPrefix) }) {
    Routing.ClaudeAny -> "claude"
    Routing.CodexAny -> "codex"
    else -> null
}

fun scheduleAccountChoice(node: NodeSummary, agent: String, picked: String?): String? {
    val named = node.logins.filter { it.agent == agent && it.state == LoginState.SignedIn && it.account != null }

    return picked?.takeIf { it.isNotEmpty() } ?: anyAccountId(agent).takeIf { named.isNotEmpty() }
}

fun routedLabel(conversation: Conversation, providers: List<ModelProvider>): String? {
    val routing = conversation.routing ?: return null
    val account = conversation.account

    return when {
        routing == Routing.AnyModel && conversation.agent == "chat" -> "Any model: ${providers.firstOrNull { it.id == conversation.providerId }?.name ?: "Chat"}${conversation.model?.let { " · $it" }.orEmpty()}"
        routing == Routing.AnyModel -> "Any model: ${agentName(conversation.agent)}${account?.let { ", $it" }.orEmpty()}"
        else -> "Any account${account?.let { ": $it" }.orEmpty()}"
    }
}

fun routedStatus(conversation: Conversation, login: LoginInfo?): String? {
    val routing = conversation.routing ?: return null
    val account = conversation.account ?: return routingName(routing)
    val worst = login?.windows?.maxByOrNull { it.usedPercent }
    val usage = worst?.let { "${(Full - it.usedPercent).roundToInt().coerceIn(0, 100)}% of the ${windowName(it.name)} left" }

    return listOfNotNull("On ${account.substringBefore('@')}", usage).joinToString(", ")
}

val PriceCurrencies = listOf("GBP" to "£", "USD" to "$", "EUR" to "€", "CAD" to "CA$", "AUD" to "A$", "CHF" to "CHF", "SEK" to "kr", "NOK" to "kr", "DKK" to "kr", "JPY" to "¥")

fun currencyName(code: String): String = PriceCurrencies.firstOrNull { it.first == code }?.let { "${it.first}, ${it.second}" } ?: code

private fun thousands(whole: Long): String = whole.toString().reversed().chunked(3).joinToString(",").reversed()

fun formatAmount(amount: Double): String {
    val cents = (amount * 100).roundToInt()
    val whole = thousands((cents / 100).toLong())

    return if (cents % 100 == 0) whole else "$whole.${(cents % 100).toString().padStart(2, '0')}"
}

fun formatMoney(amount: Double, currency: String): String {
    val symbol = PriceCurrencies.firstOrNull { it.first == currency }?.second ?: currency
    val spaced = if (symbol.length > 1 && symbol.all { it.isLetter() }) "$symbol " else symbol

    return "$spaced${formatAmount(amount)}"
}

fun periodLabel(period: String): String = if (period == "yearly") "a year" else "a month"

fun priceLabel(price: AccountPrice): String = "${formatMoney(price.amount, price.currency)} ${periodLabel(price.period)}"

private val PlainAmount = Regex("""^\d+(\.\d+)?$""")

private val GroupedAmount = Regex("""^\d{1,3}(,\d{3})+(\.\d+)?$""")

fun parseAmount(text: String): Double? {
    val trimmed = text.trim()

    return when {
        PlainAmount.matches(trimmed) -> trimmed.toDoubleOrNull()
        GroupedAmount.matches(trimmed) -> trimmed.replace(",", "").toDoubleOrNull()
        else -> null
    }?.takeIf { it > 0.0 }
}

const val AmountProblem = "Enter the amount as a number, like 1800 or 150.50"
