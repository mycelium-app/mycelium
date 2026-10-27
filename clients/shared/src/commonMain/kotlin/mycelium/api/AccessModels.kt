package mycelium.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
data class SshToken(val token: String, @SerialName("expires_at") val expiresAt: String)

@Serializable
data class ApiAccessRequest(
    val id: String,
    @SerialName("conversation_id") val conversationId: String,
    val scopes: List<String>,
    val hours: Int,
    val reason: String,
    val state: String,
    @SerialName("created_at") val createdAt: String,
    @SerialName("decided_at") val decidedAt: String? = null,
) {
    val waiting: Boolean get() = state == "requested"
}

@Serializable
data class ApproveApiAccessRequest(val scopes: List<String>? = null)

@Serializable
data class SshAccessRequest(
    val id: String,
    @SerialName("conversation_id") val conversationId: String,
    @SerialName("node_id") val nodeId: String,
    @SerialName("node_name") val nodeName: String,
    val minutes: Int,
    val reason: String,
    val state: String,
    @SerialName("created_at") val createdAt: String,
    @SerialName("decided_at") val decidedAt: String? = null,
    @SerialName("expires_at") val expiresAt: String? = null,
) {
    val waiting: Boolean get() = state == "requested"
}

@Serializable
data class ApproveSshAccessRequest(val minutes: Int? = null)
