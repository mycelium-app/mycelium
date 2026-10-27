package mycelium.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
enum class GoalState {
    @SerialName("active")
    Active,

    @SerialName("paused")
    Paused,

    @SerialName("met")
    Met,

    @SerialName("stuck")
    Stuck,

    @SerialName("over_budget")
    OverBudget,

    @SerialName("limit_reached")
    LimitReached,
}

@Serializable
data class Goal(
    @SerialName("conversation_id") val conversationId: String,
    val text: String,
    @SerialName("token_budget") val tokenBudget: Long? = null,
    @SerialName("tokens_used") val tokensUsed: Long = 0,
    val turns: Long = 0,
    val state: GoalState,
    val reason: String? = null,
    @SerialName("time_limit_seconds") val timeLimitSeconds: Long? = null,
    @SerialName("five_hour_limit") val fiveHourLimit: Int? = null,
    @SerialName("weekly_limit") val weeklyLimit: Int? = null,
    @SerialName("started_at") val startedAt: String? = null,
    @SerialName("five_hour_used") val fiveHourUsed: Double? = null,
    @SerialName("weekly_used") val weeklyUsed: Double? = null,
    @SerialName("tokens_known") val tokensKnown: Boolean = true,
    @SerialName("turn_active") val turnActive: Boolean = false,
)

@Serializable
data class SetGoalRequest(
    val text: String,
    @SerialName("token_budget") val tokenBudget: Long? = null,
    @SerialName("time_limit_seconds") val timeLimitSeconds: Long? = null,
    @SerialName("five_hour_limit") val fiveHourLimit: Int? = null,
    @SerialName("weekly_limit") val weeklyLimit: Int? = null,
)

@Serializable
data class PatchGoalRequest(val paused: Boolean)
