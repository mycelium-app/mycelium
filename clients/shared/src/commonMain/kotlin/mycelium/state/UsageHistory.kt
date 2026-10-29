package mycelium.state

import kotlin.math.roundToInt
import kotlin.math.roundToLong
import mycelium.api.UsageHistoryAccount

private val PlanNames = mapOf("pro" to "Pro", "max_5x" to "Max 5x", "max_20x" to "Max 20x", "plus" to "Plus", "team" to "Team")

fun planName(plan: String): String = PlanNames[plan] ?: plan.replace('_', ' ').replaceFirstChar { it.uppercase() }

fun approximateTokens(tokens: Double): String = "~${formatTokens(tokens.roundToLong())}"

fun approximateDollars(usd: Double): String {
    if (usd >= 10) {
        return "~$" + usd.roundToInt().toString().reversed().chunked(3).joinToString(",").reversed()
    }

    val cents = (usd * 100).roundToInt()

    return "~$${cents / 100}.${(cents % 100).toString().padStart(2, '0')}"
}

fun usageAccountName(account: UsageHistoryAccount): String = account.account.ifEmpty { agentName(account.agent) }.substringAfter('@')

fun readingsNote(account: UsageHistoryAccount): String? {
    val since = account.readingsSince?.let { dayLabel(it.take(10)) } ?: return null

    return "readings since $since"
}

fun usageEstimateNote(accounts: List<UsageHistoryAccount>): String {
    val publishedOn = accounts.mapNotNull { it.planPriceAsOf }.maxOrNull()
    val published = publishedOn?.let { ", as published on ${dayLabel(it.take(10))} ${it.take(4)}" }.orEmpty()

    return "How this is estimated: percentages are the readings each provider reports for an account. Tokens per 1% divides the tokens recorded in a window by the points it rose, counting only windows that rose 5 points or more. API equivalent prices each turn at the provider's published API rates for its model, for input, output, cache reads and cache writes$published."
}

fun sampleCount(count: Long, one: String, many: String): String = "median of $count ${if (count == 1L) one else many}"

fun priceRatio(account: UsageHistoryAccount): String? {
    val price = account.planPriceUsd ?: return null
    val equivalent = account.apiEquivalentMonthUsd ?: return null
    val tenths = (equivalent / price * 10).roundToInt()

    return "API equivalent is ${tenths / 10}.${tenths % 10} times the price"
}

fun yourPriceCaption(account: UsageHistoryAccount): String? {
    val price = account.yourPrice ?: return null
    val yearly = if (price.period == "yearly") "${formatMoney(price.amount, price.currency)} a year." else null
    val listed = account.planPriceUsd?.let { "${planName(account.plan.orEmpty())} lists $${it.toInt()}." }
    val usd = price.monthlyUsd?.takeIf { it > 0 }
    val equivalent = account.apiEquivalentMonthUsd
    val ratio = if (usd != null && equivalent != null) (equivalent / usd * 10).roundToInt().let { "API equivalent is ${it / 10}.${it % 10} times this" } else null

    return listOfNotNull(yearly, listed, ratio).joinToString(" ").ifEmpty { null }
}

fun dateTimeLabel(iso: String, localDateTime: (Long) -> String?): String {
    val moment = instantOrNull(iso) ?: return iso
    val local = localDateTime(moment.toEpochMilliseconds()) ?: moment.toString().substringBefore('Z')

    return "${dayLabel(local.substringBefore('T'))} ${local.substringAfter('T').take(5)}"
}
