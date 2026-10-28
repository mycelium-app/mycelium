package mycelium.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject

@Serializable
enum class ConversationStatus {
    @SerialName("working")
    Working,

    @SerialName("input")
    Input,

    @SerialName("review")
    Review,

    @SerialName("failed")
    Failed,

    @SerialName("done")
    Done,
}

object EventKinds {
    const val UserMessage = "user_message"
    const val AcpUpdate = "acp_update"
    const val PermissionRequest = "permission_request"
    const val PermissionResolved = "permission_resolved"
    const val ReviewRequest = "review_request"
    const val ReviewResolved = "review_resolved"
    const val TurnEnd = "turn_end"
    const val Error = "error"
    const val Workflow = "workflow"
    const val ModelChanged = "model_changed"
    const val Continued = "continued"
    const val TeachMode = "teach_mode"
    const val OpenOutline = "open_outline"
    const val GoalReport = "goal_report"
    const val Environment = "environment"
}

@Serializable
data class CreateConversationRequest(
    @SerialName("node_id") val nodeId: String,
    val agent: String,
    val account: String? = null,
    val cwd: String? = null,
    @SerialName("project_id") val projectId: String? = null,
    val branch: String? = null,
    val title: String? = null,
    val prompt: String? = null,
    val model: String? = null,
    val routing: String? = null,
)

@Serializable
data class StartSideChatRequest(
    val kind: String,
    val text: String? = null,
)

@Serializable
data class PatchConversationRequest(
    val title: String? = null,
    @SerialName("clear_title") val clearTitle: Boolean? = null,
    val archived: Boolean? = null,
    val pinned: Boolean? = null,
    val account: String? = null,
    @SerialName("use_default_account") val useDefaultAccount: Boolean? = null,
    @SerialName("provider_id") val providerId: String? = null,
    val model: String? = null,
    @SerialName("project_id") val projectId: String? = null,
    @SerialName("clear_project") val clearProject: Boolean? = null,
    @SerialName("teach_me") val teachMe: Boolean? = null,
)

@Serializable
data class ContinueRequest(
    val agent: String,
    val account: String? = null,
    @SerialName("use_default_account") val useDefaultAccount: Boolean? = null,
    @SerialName("provider_id") val providerId: String? = null,
    val model: String? = null,
    @SerialName("node_id") val nodeId: String? = null,
    val cwd: String? = null,
    val routing: String? = null,
)

@Serializable
data class SessionSummary(
    val id: String,
    val kind: String,
    val label: String,
    val hostname: String = "",
    val address: String = "",
    @SerialName("last_used_at") val lastUsedAt: String,
    val current: Boolean = false,
    val scopes: List<String> = emptyList(),
    @SerialName("expires_at") val expiresAt: String? = null,
    @SerialName("conversation_id") val conversationId: String? = null,
    @SerialName("conversation_title") val conversationTitle: String? = null,
)

@Serializable
data class Attachment(
    val sha256: String,
    val name: String,
    @SerialName("media_type") val mediaType: String,
    val size: Long,
)

@Serializable
data class PromptRequest(val text: String, val attachments: List<Attachment> = emptyList(), val spoken: Boolean = false)

@Serializable
data class PromptAccepted(val queued: Boolean, val message: String)

@Serializable
data class PatchQueueRequest(val text: String)

@Serializable
data class ResponseToEdit(
    val tool: String,
    val server: String,
    val account: String? = null,
    val filter: String? = null,
    @SerialName("filter_error") val filterError: String? = null,
    val value: JsonElement,
    @SerialName("selected_paths") val selectedPaths: List<List<JsonElement>>? = null,
)

@Serializable
data class ResponseFilterRequest(val filter: String)

@Serializable
data class ResponseFilterResult(
    @SerialName("selected_paths") val selectedPaths: List<List<JsonElement>>? = null,
    val value: JsonElement? = null,
)

@Serializable
data class SendResponseRequest(val send: JsonElement = JsonNull)

@Serializable
data class PermissionAnswer(
    @SerialName("request_id") val requestId: String,
    @SerialName("option_id") val optionId: String,
)

@Serializable
data class ConfigRequest(
    val mode: String? = null,
    val model: String? = null,
    val config: Map<String, JsonElement>? = null,
)

@Serializable
data class Conversation(
    val id: String,
    val owner: String,
    @SerialName("node_id") val nodeId: String,
    val agent: String,
    val cwd: String,
    val title: String,
    val status: ConversationStatus,
    @SerialName("acp_session_id") val acpSessionId: String? = null,
    val mode: String? = null,
    val model: String? = null,
    val options: JsonObject? = null,
    @SerialName("created_at") val createdAt: String,
    @SerialName("updated_at") val updatedAt: String,
    @SerialName("archived_at") val archivedAt: String? = null,
    @SerialName("pinned_at") val pinnedAt: String? = null,
    @SerialName("resume_at") val resumeAt: String? = null,
    val branch: String? = null,
    @SerialName("pull_request_label") val pullRequestLabel: String? = null,
    @SerialName("pull_request_url") val pullRequestUrl: String? = null,
    val account: String? = null,
    @SerialName("title_user_set") val titleUserSet: Boolean = false,
    @SerialName("parent_id") val parentId: String? = null,
    @SerialName("side_kind") val sideKind: String? = null,
    @SerialName("agent_id") val agentId: String? = null,
    @SerialName("provider_id") val providerId: String? = null,
    @SerialName("project_id") val projectId: String? = null,
    @SerialName("group_id") val groupId: String? = null,
    @SerialName("assistant_id") val assistantId: String? = null,
    @SerialName("agent_active") val agentActive: Boolean = false,
    @SerialName("teach_me") val teachMe: Boolean = false,
    val routing: String? = null,
    val scratch: Boolean = false,
)

@Serializable
data class Event(
    @SerialName("conversation_id") val conversationId: String,
    val seq: Long,
    val kind: String,
    val payload: JsonElement = JsonObject(emptyMap()),
)

@Serializable
enum class MoveState {
    @SerialName("waiting")
    Waiting,

    @SerialName("moving")
    Moving,

    @SerialName("done")
    Done,

    @SerialName("failed")
    Failed,

    @SerialName("cancelled")
    Cancelled,
}

@Serializable
data class MoveRequest(@SerialName("node_id") val nodeId: String, val fork: Boolean = false)

@Serializable
data class MovePlan(
    val movable: Boolean,
    val reason: String? = null,
    val branch: String? = null,
    @SerialName("changed_files") val changedFiles: Int = 0,
    @SerialName("project_on_target") val projectOnTarget: Boolean = true,
    @SerialName("clones_project") val clonesProject: Boolean = false,
    @SerialName("agent_session") val agentSession: Boolean = false,
    @SerialName("stays_ports") val staysPorts: List<Int> = emptyList(),
    @SerialName("stays_processes") val staysProcesses: List<String> = emptyList(),
)

@Serializable
data class MoveProgress(
    @SerialName("conversation_id") val conversationId: String,
    @SerialName("node_id") val nodeId: String,
    val fork: Boolean = false,
    val state: MoveState,
    val message: String? = null,
    @SerialName("result_id") val resultId: String? = null,
)

@Serializable
data class QueuedPrompt(
    val id: String,
    @SerialName("conversation_id") val conversationId: String,
    val position: Int,
    val text: String,
    @SerialName("created_at") val createdAt: String,
    val attachments: List<Attachment> = emptyList(),
)
