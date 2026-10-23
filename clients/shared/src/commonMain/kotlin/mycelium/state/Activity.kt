package mycelium.state

import mycelium.api.Activity
import kotlin.math.roundToInt

val ActivityRanges = listOf("Today" to "today", "7 days" to "7d", "30 days" to "30d")

private val Months = listOf("Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec")

fun formatTokens(tokens: Long): String = when {
    tokens < 1_000 -> tokens.toString()
    tokens < 1_000_000 -> "${withOneDecimal((tokens / 100.0).roundToInt())}k"
    tokens < 1_000_000_000 -> "${withOneDecimal((tokens / 100_000.0).roundToInt())}M"
    else -> "${withOneDecimal((tokens / 100_000_000.0).roundToInt())}B"
}

fun formatCost(usd: Double?): String {
    if (usd == null) {
        return "Not reported"
    }

    if (usd >= 100) {
        return "~$" + usd.roundToInt().toString().reversed().chunked(3).joinToString(",").reversed()
    }

    val cents = (usd * 100).roundToInt()

    return "~$${cents / 100}.${(cents % 100).toString().padStart(2, '0')}"
}

fun formatDuration(milliseconds: Long): String {
    val seconds = milliseconds / 1000

    return when {
        seconds < 60 -> "${seconds}s"
        seconds < 3600 -> "${seconds / 60}m ${seconds % 60}s"
        else -> "${seconds / 3600}h ${(seconds % 3600) / 60}m"
    }
}

fun dayLabel(day: String): String {
    val parts = day.split('-')
    val month = parts.getOrNull(1)?.toIntOrNull()?.let { Months.getOrNull(it - 1) } ?: return day

    return "${parts[2].trimStart('0')} $month"
}

fun usageNotice(activity: Activity): String? {
    val names = activity.unreportedAgents.map(::agentName)

    return when {
        activity.totals.turns == 0L -> null
        activity.totals.reportedTurns == 0L -> "No agent reported token usage in this range, so only turns and time working are counted."
        names.isNotEmpty() -> "${names.joinToString(" and ")} did not report token usage in this range, so only ${if (names.size == 1) "its" else "their"} turns and time working are counted."
        else -> null
    }
}

fun tokensPerDaySummary(activity: Activity): String {
    val peak = activity.days.maxByOrNull { it.tokens }

    return if (peak == null || peak.tokens == 0L) "No tokens were reported on these days." else "Peak ${formatTokens(peak.tokens)} on ${dayLabel(peak.key)}, ${formatTokens(activity.totals.tokens)} in total."
}
