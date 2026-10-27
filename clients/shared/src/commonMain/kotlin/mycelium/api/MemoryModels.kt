package mycelium.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
data class Memory(
    val id: String,
    val text: String,
    val scopes: List<String> = emptyList(),
    @SerialName("created_by") val createdBy: String,
    @SerialName("conversation_id") val conversationId: String? = null,
    @SerialName("bank_id") val bankId: String? = null,
    val source: MemorySource = MemorySource(),
    @SerialName("created_at") val createdAt: String,
    @SerialName("updated_at") val updatedAt: String,
)

@Serializable
data class MemoryBank(
    val id: String,
    val name: String,
    @SerialName("model_class") val modelClass: String,
    @SerialName("group_ids") val groupIds: List<String> = emptyList(),
    @SerialName("created_at") val createdAt: String,
)

@Serializable
data class SaveMemoryBankRequest(
    val name: String,
    @SerialName("model_class") val modelClass: String,
    @SerialName("group_ids") val groupIds: List<String> = emptyList(),
)

@Serializable
data class MoveMemoryRequest(@SerialName("bank_id") val bankId: String?)

@Serializable
data class MemorySource(
    val model: String? = null,
    val agent: String? = null,
    @SerialName("node_name") val nodeName: String? = null,
    val account: String? = null,
    @SerialName("project_name") val projectName: String? = null,
    @SerialName("conversation_title") val conversationTitle: String? = null,
    @SerialName("message_seq") val messageSeq: Long? = null,
    @SerialName("tool_call_id") val toolCallId: String? = null,
) {
    val summary: String get() = listOfNotNull(model, agent, nodeName, account, projectName).joinToString("  ·  ")
}

@Serializable
data class UpdateMemoryRequest(val text: String)
