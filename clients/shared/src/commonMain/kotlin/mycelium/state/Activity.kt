package mycelium.state

import mycelium.api.Activity
import mycelium.api.ActivityRow
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

fun tokensPerHourSummary(activity: Activity): String {
    val peak = activity.hours.maxByOrNull { it.tokens }

    if (peak == null || peak.tokens == 0L) {
        return "No tokens were reported in these hours."
    }

    return "Peak ${formatTokens(peak.tokens)} in the hour from ${dayLabel(peak.key.take(10))} ${peak.key.takeLast(2)}:00, ${formatTokens(activity.totals.tokens)} in total."
}

fun tokensPerHourTitle(range: String): String = if (range == "today") "Tokens per hour, today" else "Tokens per hour, last ${ActivityRanges.first { it.second == range }.first}"

const val WhPerThousandOutputTokens = 0.6
const val WhPerThousandInputTokens = 0.2
const val MillilitresWaterPerWh = 1.1
const val GramsCo2PerKwh = 445.0
const val GramsCo2PerTreeDay = 60.0

const val EstimateNote = "How this is estimated: figures are derived from token counts, never measured. " +
    "Energy uses 0.6 Wh per 1,000 output tokens and 0.2 Wh per 1,000 input and cached tokens (Epoch AI, 2025). " +
    "Water uses 1.1 mL per Wh (Google, 2025). CO\u2082 uses 445 g per kWh (IEA, 2024). " +
    "A tree-day is the 60 g of CO\u2082 a mature tree absorbs in a day, about 22 kg a year, a widely quoted figure."

data class UsageEstimate(val energyWh: Double, val waterMl: Double, val co2Grams: Double, val treeDays: Double)

fun estimateUsage(row: ActivityRow): UsageEstimate {
    val energyWh = row.outputTokens / 1000.0 * WhPerThousandOutputTokens + (row.inputTokens + row.cacheReadTokens + row.cacheWriteTokens) / 1000.0 * WhPerThousandInputTokens
    val co2Grams = energyWh / 1000 * GramsCo2PerKwh

    return UsageEstimate(energyWh, energyWh * MillilitresWaterPerWh, co2Grams, co2Grams / GramsCo2PerTreeDay)
}

private fun approximately(value: Double): String = "~" + if (value >= 10) value.roundToInt().toString() else withOneDecimal((value * 10).roundToInt())

fun formatEnergy(wh: Double): String = if (wh >= 1000) "${approximately(wh / 1000)} kWh" else "${approximately(wh)} Wh"

fun formatWater(ml: Double): String = if (ml >= 1000) "${approximately(ml / 1000)} L of water" else "${approximately(ml)} mL of water"

fun formatCo2(grams: Double): String = if (grams >= 1000) "${approximately(grams / 1000)} kg CO\u2082" else "${approximately(grams)} g CO\u2082"

fun formatTreeDays(days: Double): String = if (days < 1) "<1 tree-day" else "~${days.roundToInt()} ${if (days.roundToInt() == 1) "tree-day" else "tree-days"}"
