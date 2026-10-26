package mycelium.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
data class WalkthroughCheck(val step: Int, val question: String, val choices: List<String>)

@Serializable
data class WalkthroughAnswerRequest(val choice: Int)

@Serializable
data class WalkthroughAnswer(val step: Int, val choice: Int, val correct: Boolean, val explanation: String = "")

@Serializable
data class Reflection(
    val id: String,
    val week: String,
    val done: String = "",
    val learned: String = "",
    val waiting: String = "",
    @SerialName("next_week") val nextWeek: String = "",
    val edited: List<String> = emptyList(),
    @SerialName("drafted_at") val draftedAt: String? = null,
    @SerialName("conversation_id") val conversationId: String? = null,
)

@Serializable
data class PatchReflectionRequest(
    val done: String? = null,
    val learned: String? = null,
    val waiting: String? = null,
    @SerialName("next_week") val nextWeek: String? = null,
)

@Serializable
data class ReflectionScheduleRequest(
    @SerialName("node_id") val nodeId: String,
    val agent: String,
    val cwd: String,
    val enabled: Boolean,
)

@Serializable
data class ReflectionSchedule(
    @SerialName("schedule_id") val scheduleId: String,
    @SerialName("node_id") val nodeId: String,
    val agent: String,
    val cwd: String,
    val enabled: Boolean,
    val cron: String = "",
)

@Serializable
data class RedraftReflection(@SerialName("conversation_id") val conversationId: String)
