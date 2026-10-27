package mycelium.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
data class Task(
    val id: String,
    @SerialName("node_id") val nodeId: String,
    val agent: String,
    val cwd: String,
    @SerialName("project_id") val projectId: String? = null,
    val branch: String? = null,
    val title: String,
    val prompt: String? = null,
    @SerialName("created_at") val createdAt: String,
    val status: String = TaskStatuses.Open,
    val options: List<String> = emptyList(),
    val answer: String? = null,
    @SerialName("source_id") val sourceId: String? = null,
    @SerialName("source_title") val sourceTitle: String? = null,
    @SerialName("conversation_id") val conversationId: String? = null,
    @SerialName("depends_on") val dependsOn: List<String> = emptyList(),
)

object TaskStatuses {
    const val Question = "question"
    const val Proposed = "proposed"
    const val Open = "open"
    const val Claimed = "claimed"
    const val Done = "done"
    const val Declined = "declined"
}

@Serializable
data class AnswerTaskRequest(val answer: String)

@Serializable
data class StartTaskRequest(@SerialName("conversation_id") val conversationId: String)

@Serializable
data class CreateTaskRequest(
    @SerialName("node_id") val nodeId: String,
    val agent: String,
    val title: String,
    val cwd: String? = null,
    @SerialName("project_id") val projectId: String? = null,
    val branch: String? = null,
    val prompt: String? = null,
    @SerialName("depends_on") val dependsOn: List<String> = emptyList(),
)
