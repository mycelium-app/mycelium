package mycelium.state

import kotlin.math.absoluteValue
import kotlin.math.roundToLong
import kotlin.time.Clock
import kotlin.time.Instant
import mycelium.api.ModelProviderUsage

data class SpendFigure(val amount: String, val label: String)

data class ApiSpendRow(val name: String, val today: SpendFigure?, val month: SpendFigure?, val problem: String?, val tooltip: String)

fun usdAmount(amount: Double): String {
    val cents = (amount * 100).roundToLong()
    val sign = if (cents < 0) "-" else ""
    val absolute = cents.absoluteValue

    return "$sign$${absolute / 100}.${(absolute % 100).toString().padStart(2, '0')}"
}

fun apiSpendRow(provider: ModelProviderUsage, now: Instant = Clock.System.now()): ApiSpendRow {
    val monthName = monthAbbreviation(now.toString().substring(5, 7).toInt())
    val name = if (provider.apiCost != null) provider.name else if (provider.name.equals("OpenRouter", ignoreCase = true)) "OpenRouter" else "OpenRouter: ${provider.name}"
    val cost = provider.apiCost
    val spend = provider.spend
    val today = cost?.todayUsd ?: spend?.daily
    val month = cost?.monthUsd ?: spend?.monthly
    val total = spend?.total
    val read = today != null || month != null || total != null
    val problem = when {
        provider.status == "refused" -> "Admin key refused"
        !read && provider.status == "unreadable" -> "Could not be read"
        !read -> "Not reported yet"
        else -> null
    }
    val tooltip = when {
        problem != null -> problem
        cost != null -> "Today ${usdAmount(cost.todayUsd)} and ${tokenCount(cost.todayTokens)} tokens. So far this month ${usdAmount(cost.monthUsd)}." + if (provider.status == "unreadable") " Could not be read just now." else ""
        else -> listOfNotNull(today?.let { "Today ${usdAmount(it)}." }, month?.let { "So far this month ${usdAmount(it)}." }, total?.let { "In total ${usdAmount(it)}." }).joinToString(" ")
    }

    return ApiSpendRow(
        name,
        today?.let { SpendFigure(usdAmount(it), "today") } ?: if (month == null) total?.let { SpendFigure(usdAmount(it), "in total") } else null,
        month?.let { SpendFigure(usdAmount(it), "in $monthName") },
        problem,
        tooltip,
    )
}

const val AdminKeyPrefix = "sk-ant-admin"

const val AdminKeyProblem = "Enter an Admin API key that starts with $AdminKeyPrefix"
