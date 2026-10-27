package mycelium.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
data class ResetCredits(
    @SerialName("available_count") val availableCount: Int,
    val credits: List<ResetCredit>? = null,
)

@Serializable
data class ResetCredit(
    val id: String,
    @SerialName("expires_at") val expiresAt: String? = null,
    val title: String? = null,
)

@Serializable
data class UseResetRequest(
    val agent: String,
    val account: String? = null,
    @SerialName("idempotency_key") val idempotencyKey: String,
)

@Serializable
data class UseResetResponse(val outcome: String)

@Serializable
data class UsageWindow(
    val name: String,
    @SerialName("used_percent") val usedPercent: Double,
    @SerialName("resets_at") val resetsAt: String? = null,
    @SerialName("rate_per_hour") val ratePerHour: Double? = null,
    @SerialName("runs_out_at") val runsOutAt: String? = null,
)

@Serializable
data class ConversationUsage(
    @SerialName("context_used") val contextUsed: Long? = null,
    @SerialName("context_size") val contextSize: Long? = null,
    @SerialName("input_tokens") val inputTokens: Long = 0,
    @SerialName("output_tokens") val outputTokens: Long = 0,
    @SerialName("cache_read_tokens") val cacheReadTokens: Long = 0,
    @SerialName("cache_write_tokens") val cacheWriteTokens: Long = 0,
    @SerialName("cost_usd") val costUsd: Double? = null,
    @SerialName("cost_estimated") val costEstimated: Boolean = false,
) {
    val tokens: Long get() = inputTokens + outputTokens + cacheReadTokens + cacheWriteTokens
}

@Serializable
data class ActivityRow(
    val key: String,
    val name: String,
    val turns: Long = 0,
    @SerialName("reported_turns") val reportedTurns: Long = 0,
    @SerialName("working_ms") val workingMs: Long = 0,
    @SerialName("input_tokens") val inputTokens: Long = 0,
    @SerialName("output_tokens") val outputTokens: Long = 0,
    @SerialName("cache_read_tokens") val cacheReadTokens: Long = 0,
    @SerialName("cache_write_tokens") val cacheWriteTokens: Long = 0,
    @SerialName("subagent_tokens") val subagentTokens: Long = 0,
    @SerialName("cost_usd") val costUsd: Double? = null,
    @SerialName("cost_estimated") val costEstimated: Boolean = false,
) {
    val tokens: Long get() = inputTokens + outputTokens + cacheReadTokens + cacheWriteTokens
}

@Serializable
data class Activity(
    val range: String,
    val totals: ActivityRow,
    val days: List<ActivityRow> = emptyList(),
    val hours: List<ActivityRow> = emptyList(),
    val conversations: List<ActivityRow> = emptyList(),
    val nodes: List<ActivityRow> = emptyList(),
    val agents: List<ActivityRow> = emptyList(),
    val projects: List<ActivityRow> = emptyList(),
    val models: List<ActivityRow> = emptyList(),
    @SerialName("unreported_agents") val unreportedAgents: List<String> = emptyList(),
)

@Serializable
data class ModelProviderUsage(
    @SerialName("provider_id") val providerId: String,
    val name: String,
    @SerialName("read_at") val readAt: String? = null,
    val spend: OpenRouterSpend? = null,
)

@Serializable
data class OpenRouterSpend(
    val total: Double,
    val daily: Double? = null,
    val weekly: Double? = null,
    val monthly: Double? = null,
    @SerialName("limit_remaining") val limitRemaining: Double? = null,
    val limit: Double? = null,
    @SerialName("limit_reset") val limitReset: String? = null,
)

@Serializable
data class UsageLimitSetting(@SerialName("on_usage_limit") val onUsageLimit: String)
