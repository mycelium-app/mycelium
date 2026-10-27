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
data class RedraftReflection(@SerialName("conversation_id") val conversationId: String)

@Serializable
data class Skill(
    val kind: String,
    val name: String,
    @SerialName("own_lines") val ownLines: Long,
    @SerialName("agent_lines") val agentLines: Long,
    @SerialName("own_share") val ownShare: Double? = null,
    @SerialName("weekly_own_lines") val weeklyOwnLines: List<Long> = emptyList(),
    @SerialName("last_practised") val lastPractised: String? = null,
    val hidden: Boolean = false,
)

@Serializable
data class HiddenSkillRequest(val kind: String, val name: String)

@Serializable
data class ExerciseTests(
    val exit: Int = 0,
    val passed: Int? = null,
    val failed: Int = 0,
    val failing: List<String> = emptyList(),
    @SerialName("timed_out") val timedOut: Boolean = false,
    @SerialName("output_tail") val outputTail: String = "",
)

@Serializable
data class ReviewComment(val path: String, val line: Int, val body: String, val praise: Boolean = false)

@Serializable
data class ExerciseReview(
    val id: String,
    val summary: String,
    val comments: List<ReviewComment> = emptyList(),
    val tests: ExerciseTests? = null,
    @SerialName("created_at") val createdAt: String,
)

@Serializable
data class Exercise(
    val id: String,
    val kind: String,
    val name: String,
    val title: String,
    val brief: String,
    val minutes: Int? = null,
    val state: String,
    val branch: String? = null,
    @SerialName("conversation_id") val conversationId: String? = null,
    val reviewing: Boolean = false,
    val tests: ExerciseTests? = null,
    val review: ExerciseReview? = null,
    @SerialName("created_at") val createdAt: String,
    @SerialName("started_at") val startedAt: String? = null,
    @SerialName("source_conversation_id") val sourceConversationId: String? = null,
    @SerialName("source_title") val sourceTitle: String? = null,
)

@Serializable
data class SolutionFile(val path: String, val diff: String)
