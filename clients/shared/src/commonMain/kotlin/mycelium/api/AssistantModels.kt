package mycelium.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
data class ModelChoice(
    val agent: String? = null,
    @SerialName("provider_id") val providerId: String? = null,
    val model: String? = null,
    val account: String? = null,
    @SerialName("any_model") val anyModel: Boolean = false,
)

@Serializable
data class AssistantPreferences(
    val models: List<ModelChoice> = emptyList(),
    val nodes: List<String> = emptyList(),
    @SerialName("chat_first") val chatFirst: Boolean = true,
    @SerialName("fall_back_on") val fallBackOn: List<String> = listOf("usage_limit", "signed_out", "offline"),
    @SerialName("voice_name") val voiceName: String? = null,
    @SerialName("speech_speed") val speechSpeed: String? = null,
)

@Serializable
data class Assistant(
    val id: String,
    val name: String,
    val description: String? = null,
    @SerialName("group_id") val groupId: String? = null,
    val preferences: AssistantPreferences = AssistantPreferences(),
    @SerialName("conversation_id") val conversationId: String? = null,
    val state: String = "idle",
    @SerialName("last_active_at") val lastActiveAt: String? = null,
    @SerialName("session_count") val sessionCount: Long = 0,
    @SerialName("pinned_at") val pinnedAt: String? = null,
    @SerialName("archived_at") val archivedAt: String? = null,
    val instructions: String? = null,
    @SerialName("next_choice") val nextChoice: String? = null,
    val secrets: List<AssistantSecret> = emptyList(),
    val backup: AssistantBackup? = null,
    val wakes: String? = null,
    @SerialName("builtin_key") val builtinKey: String? = null,
    @SerialName("created_at") val createdAt: String,
)

@Serializable
data class ActionInput(
    val name: String,
    val kind: String = "text",
    val options: List<String> = emptyList(),
    val required: Boolean = false,
    val default: String? = null,
)

@Serializable
data class AssistantAction(
    val id: String,
    @SerialName("assistant_id") val assistantId: String,
    val name: String,
    val prompt: String,
    val inputs: List<ActionInput> = emptyList(),
    val model: ModelChoice? = null,
    @SerialName("node_id") val nodeId: String? = null,
    val pinned: Boolean = false,
    val position: Int = 0,
    @SerialName("builtin_key") val builtinKey: String? = null,
)

@Serializable
data class SaveActionRequest(
    val name: String,
    val prompt: String,
    val inputs: List<ActionInput> = emptyList(),
    val model: ModelChoice? = null,
    @SerialName("node_id") val nodeId: String? = null,
    val pinned: Boolean = false,
)

@Serializable
data class RunActionRequest(val inputs: Map<String, String> = emptyMap())

@Serializable
data class ActionProposal(
    val id: String,
    @SerialName("assistant_id") val assistantId: String,
    @SerialName("conversation_id") val conversationId: String? = null,
    val name: String,
    val prompt: String,
    val inputs: List<ActionInput> = emptyList(),
    val reason: String = "",
    val state: String,
    @SerialName("created_at") val createdAt: String,
)

@Serializable
data class AssistantChange(
    val id: String,
    @SerialName("conversation_id") val conversationId: String? = null,
    val kind: String,
    @SerialName("assistant_id") val assistantId: String? = null,
    val name: String,
    val description: String? = null,
    @SerialName("group_id") val groupId: String? = null,
    val instructions: String? = null,
    val preferences: AssistantPreferences? = null,
    val state: String,
    @SerialName("created_at") val createdAt: String,
)

@Serializable
data class AssistantSecret(val name: String, val description: String? = null, val state: String, @SerialName("set_by") val setBy: String? = null)

@Serializable
data class PutAssistantSecretRequest(val value: String? = null, val description: String? = null)

@Serializable
data class AssistantFile(val path: String, val size: Long, @SerialName("updated_at") val updatedAt: String, @SerialName("backed_up") val backedUp: Boolean = false)

@Serializable
data class AssistantBackup(
    @SerialName("forge_id") val forgeId: String,
    val repository: String,
    val branch: String? = null,
    @SerialName("last_at") val lastAt: String? = null,
    @SerialName("last_commit") val lastCommit: String? = null,
    val error: String? = null,
)

@Serializable
data class SetAssistantBackupRequest(@SerialName("forge_id") val forgeId: String, val repository: String, val branch: String? = null)

@Serializable
data class CreateAssistantRequest(
    val name: String,
    val description: String? = null,
    @SerialName("group_id") val groupId: String? = null,
    val instructions: String? = null,
    val preferences: AssistantPreferences? = null,
)

@Serializable
data class PatchAssistantRequest(
    val name: String? = null,
    val description: String? = null,
    @SerialName("group_id") val groupId: String? = null,
    @SerialName("clear_group") val clearGroup: Boolean = false,
    val instructions: String? = null,
    val pinned: Boolean? = null,
    val archived: Boolean? = null,
    val preferences: AssistantPreferences? = null,
)
