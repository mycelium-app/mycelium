package mycelium.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
data class Schedule(
    val id: String,
    @SerialName("node_id") val nodeId: String? = null,
    val agent: String? = null,
    val account: String? = null,
    val routing: String? = null,
    val model: String? = null,
    val cwd: String,
    @SerialName("project_id") val projectId: String? = null,
    val prompt: String,
    val cron: String,
    @SerialName("each_run") val eachRun: String,
    val paused: Boolean,
    @SerialName("time_zone") val timeZone: String = "UTC",
    val proposed: Boolean = false,
    @SerialName("conversation_id") val conversationId: String? = null,
    @SerialName("next_run_at") val nextRunAt: String,
    @SerialName("last_run_at") val lastRunAt: String? = null,
    @SerialName("last_conversation_id") val lastConversationId: String? = null,
    @SerialName("assistant_id") val assistantId: String? = null,
    @SerialName("action_id") val actionId: String? = null,
    @SerialName("action_inputs") val actionInputs: Map<String, String> = emptyMap(),
    @SerialName("created_at") val createdAt: String,
)

@Serializable
data class CreateScheduleRequest(
    @SerialName("node_id") val nodeId: String? = null,
    val agent: String? = null,
    val account: String? = null,
    val routing: String? = null,
    val model: String? = null,
    val prompt: String,
    val cron: String,
    @SerialName("each_run") val eachRun: String,
    val cwd: String? = null,
    @SerialName("project_id") val projectId: String? = null,
    @SerialName("time_zone") val timeZone: String? = null,
    @SerialName("assistant_id") val assistantId: String? = null,
    @SerialName("action_id") val actionId: String? = null,
    @SerialName("action_inputs") val actionInputs: Map<String, String> = emptyMap(),
)

@Serializable
data class PatchScheduleRequest(
    val paused: Boolean? = null,
    val prompt: String? = null,
    val cron: String? = null,
    @SerialName("each_run") val eachRun: String? = null,
    @SerialName("time_zone") val timeZone: String? = null,
)

@Serializable
data class ConfirmScheduleRequest(
    val cron: String? = null,
    @SerialName("each_run") val eachRun: String? = null,
    @SerialName("time_zone") val timeZone: String? = null,
)

@Serializable
data class SetTimeZoneRequest(@SerialName("time_zone") val timeZone: String)

@Serializable
data class ScheduleRun(
    val id: String,
    @SerialName("schedule_id") val scheduleId: String,
    @SerialName("ran_at") val ranAt: String,
    val outcome: String,
    val reason: String? = null,
    @SerialName("conversation_id") val conversationId: String? = null,
)

object EachRun {
    const val New = "new"
    const val Same = "same"
}
