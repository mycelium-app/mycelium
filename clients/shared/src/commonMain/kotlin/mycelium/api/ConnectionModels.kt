package mycelium.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
data class ConversationConnection(
    val id: String,
    @SerialName("source_id") val sourceId: String,
    @SerialName("target_id") val targetId: String,
    @SerialName("source_title") val sourceTitle: String,
    @SerialName("target_title") val targetTitle: String,
    @SerialName("created_at") val createdAt: String,
    @SerialName("revoked_at") val revokedAt: String? = null,
)

@Serializable
data class CreateConversationConnectionRequest(
    @SerialName("source_id") val sourceId: String,
    @SerialName("target_id") val targetId: String,
)

@Serializable
data class GroupConnection(
    val id: String,
    @SerialName("source_group_id") val sourceGroupId: String,
    @SerialName("target_group_id") val targetGroupId: String,
    @SerialName("source_name") val sourceName: String,
    @SerialName("target_name") val targetName: String,
    @SerialName("created_at") val createdAt: String,
    @SerialName("revoked_at") val revokedAt: String? = null,
)

@Serializable
data class CreateGroupConnectionRequest(
    @SerialName("source_group_id") val sourceGroupId: String,
    @SerialName("target_group_id") val targetGroupId: String,
)

@Serializable
data class MessageProposal(
    val id: String,
    @SerialName("conversation_id") val conversationId: String? = null,
    @SerialName("target_id") val targetId: String,
    @SerialName("target_title") val targetTitle: String,
    @SerialName("target_node") val targetNode: String,
    @SerialName("target_project") val targetProject: String? = null,
    val message: String,
    val reason: String,
    val state: String,
    @SerialName("created_at") val createdAt: String,
    @SerialName("decided_at") val decidedAt: String? = null,
    @SerialName("source_group") val sourceGroup: String? = null,
    @SerialName("target_group") val targetGroup: String? = null,
) {
    val waiting: Boolean get() = state == "proposed"
}
