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

    @SerialName("stalled")
    Stalled,
}

@Serializable
enum class GoalWait {
    @SerialName("repeated_outcome")
    RepeatedOutcome,

    @SerialName("question_waiting")
    QuestionWaiting,

    @SerialName("background_work")
    BackgroundWork,
}

@Serializable
enum class GoalDelivery {
    @SerialName("pending")
    Pending,

    @SerialName("sent")
    Sent,

    @SerialName("failed")
    Failed,
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
    val delivery: GoalDelivery = GoalDelivery.Sent,
    @SerialName("delivered_at") val deliveredAt: String? = null,
    @SerialName("delivery_reason") val deliveryReason: String? = null,
    @SerialName("held_since") val heldSince: String? = null,
    @SerialName("held_seconds") val heldSeconds: Long = 0,
    @SerialName("resumes_at") val resumesAt: String? = null,
    val nudges: Long = 0,
    val repeats: Long = 0,
    @SerialName("repeat_limit") val repeatLimit: Long? = null,
    @SerialName("next_nudge_at") val nextNudgeAt: String? = null,
    @SerialName("wait_reason") val waitReason: GoalWait? = null,
    @SerialName("reported_at") val reportedAt: String? = null,
)

@Serializable
data class SetGoalRequest(
    val text: String,
    @SerialName("token_budget") val tokenBudget: Long? = null,
    @SerialName("time_limit_seconds") val timeLimitSeconds: Long? = null,
    @SerialName("five_hour_limit") val fiveHourLimit: Int? = null,
    @SerialName("weekly_limit") val weeklyLimit: Int? = null,
    @SerialName("repeat_limit") val repeatLimit: Int? = null,
)

@Serializable
data class PatchGoalRequest(val paused: Boolean)

@Serializable
data class ContinueGoalRequest(val note: String? = null)
