package mycelium.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
data class ProbeChatToolServerRequest(val url: String)

@Serializable
data class ChatToolServerProbe(val oauth: Boolean = false)

@Serializable
data class TestChatToolServerRequest(val url: String, val credential: String? = null)

@Serializable
data class TestedTool(val name: String, val kind: String = "write")

@Serializable
data class TestedChatToolServer(val tools: List<TestedTool> = emptyList())

@Serializable
data class IntegrationApiGrant(
    @SerialName("token_id") val tokenId: String,
    @SerialName("conversation_id") val conversationId: String? = null,
    @SerialName("conversation_title") val conversationTitle: String? = null,
    val scopes: List<String> = emptyList(),
    @SerialName("expires_at") val expiresAt: String,
    val calls: Long = 0,
)
