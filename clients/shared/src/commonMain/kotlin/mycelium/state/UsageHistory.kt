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
    val orchestrator = account.orchestratorUntil?.let { ", before ${dayLabel(it.take(10))} from orchestrator" }.orEmpty()

    return "readings since $since$orchestrator"
}

fun sampleCount(count: Long, one: String, many: String): String = "median of $count ${if (count == 1L) one else many}"

fun priceRatio(account: UsageHistoryAccount): String? {
    val price = account.planPriceUsd ?: return null
    val equivalent = account.apiEquivalentMonthUsd ?: return null
    val tenths = (equivalent / price * 10).roundToInt()

    return "API equivalent is ${tenths / 10}.${tenths % 10} times the price"
}

fun dateTimeLabel(iso: String, localDateTime: (Long) -> String?): String {
    val moment = runCatching { kotlin.time.Instant.parse(iso) }.getOrNull() ?: return iso
    val local = localDateTime(moment.toEpochMilliseconds()) ?: moment.toString().substringBefore('Z')

    return "${dayLabel(local.substringBefore('T'))} ${local.substringAfter('T').take(5)}"
}
