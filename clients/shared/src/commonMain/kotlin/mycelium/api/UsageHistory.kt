package mycelium.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
data class UsageHistory(val accounts: List<UsageHistoryAccount> = emptyList())

@Serializable
data class UsageHistoryAccount(
    val agent: String,
    val account: String,
    val plan: String? = null,
    @SerialName("plan_price_usd") val planPriceUsd: Double? = null,
    @SerialName("plan_price_as_of") val planPriceAsOf: String? = null,
    @SerialName("your_price") val yourPrice: YourPrice? = null,
    @SerialName("readings_since") val readingsSince: String? = null,
    val readings: List<UsageReading> = emptyList(),
    val windows: List<UsageHistoryWindow> = emptyList(),
    @SerialName("tokens_per_point") val tokensPerPoint: TokensPerPoint = TokensPerPoint(),
    @SerialName("api_equivalent_month_usd") val apiEquivalentMonthUsd: Double? = null,
    @SerialName("api_equivalent_per_window_usd") val apiEquivalentPerWindowUsd: Double? = null,
)

@Serializable
data class UsageReading(val at: String, val window: String, val percent: Double)

@Serializable
data class UsageHistoryWindow(
    val window: String,
    @SerialName("started_at") val startedAt: String,
    @SerialName("peak_percent") val peakPercent: Double,
    val tokens: Long = 0,
    @SerialName("tokens_per_point") val tokensPerPoint: Double? = null,
    @SerialName("api_equivalent_usd") val apiEquivalentUsd: Double? = null,
)

@Serializable
data class TokensPerPoint(
    @SerialName("five_hour") val fiveHour: Double? = null,
    @SerialName("seven_day") val sevenDay: Double? = null,
    @SerialName("five_hour_samples") val fiveHourSamples: Long = 0,
    @SerialName("seven_day_samples") val sevenDaySamples: Long = 0,
)
