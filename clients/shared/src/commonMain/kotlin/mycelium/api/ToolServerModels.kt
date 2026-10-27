package mycelium.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonObject

@Serializable
data class ChatToolInfo(
    val name: String,
    val description: String = "",
    val kind: String = "write",
    val asks: Boolean = false,
    val edits: Boolean = false,
    @SerialName("ask_source") val askSource: String = "preset",
    @SerialName("edit_source") val editSource: String = "preset",
)

@Serializable
data class PolicyLevel(
    val read: Boolean? = null,
    val write: Boolean? = null,
    val tools: Map<String, Boolean> = emptyMap(),
)

@Serializable
data class IntegrationPolicy(val preset: String = PresetAskWrites, val ask: PolicyLevel = PolicyLevel(), val edit: PolicyLevel = PolicyLevel())

const val PresetAskWrites = "ask_writes"
const val PresetAskAll = "ask_all"
const val PresetAskAllEdit = "ask_all_edit"
const val PresetDontAsk = "dont_ask"

@Serializable
data class IntegrationCall(
    val id: String,
    @SerialName("server_id") val serverId: String? = null,
    @SerialName("server_name") val serverName: String,
    val tool: String,
    val kind: String = "write",
    @SerialName("conversation_id") val conversationId: String? = null,
    val surface: String,
    val provider: String? = null,
    val decision: String,
    val review: String? = null,
    @SerialName("is_error") val isError: Boolean = false,
    val at: String,
)

@Serializable
data class SetIntegrationGroupsRequest(val groups: List<String>)

@Serializable
data class ChatToolServer(
    val id: String,
    val name: String,
    val kind: String = "mcp",
    val url: String,
    val account: String? = null,
    val settings: JsonObject? = null,
    @SerialName("token_hint") val tokenHint: String? = null,
    val oauth: Boolean = false,
    val enabled: Boolean = true,
    val tools: List<ChatToolInfo> = emptyList(),
    @SerialName("created_at") val createdAt: String,
    @SerialName("signed_in") val signedIn: Boolean = false,
    @SerialName("forge_id") val forgeId: String? = null,
    val status: String = "connected",
    @SerialName("token_expires_at") val tokenExpiresAt: String? = null,
    @SerialName("last_used_at") val lastUsedAt: String? = null,
    @SerialName("last_error") val lastError: String? = null,
    val policy: IntegrationPolicy = IntegrationPolicy(),
    val groups: List<String> = emptyList(),
)

@Serializable
data class CreateChatToolServerRequest(val name: String, val url: String, @SerialName("bearer_token") val bearerToken: String? = null, val kind: String? = null, val account: String? = null, val settings: JsonObject? = null, @SerialName("token_expires_at") val tokenExpiresAt: String? = null)

@Serializable
data class StartChatToolServerOAuthRequest(val name: String, val url: String, val kind: String? = null, @SerialName("forge_id") val forgeId: String? = null)

@Serializable
data class OAuthStart(@SerialName("authorize_url") val authorizeUrl: String)

@Serializable
data class PatchChatToolServerRequest(val enabled: Boolean? = null, val name: String? = null, @SerialName("token_expires_at") val tokenExpiresAt: String? = null, @SerialName("clear_token_expiry") val clearTokenExpiry: Boolean? = null, val policy: IntegrationPolicy? = null, @SerialName("bearer_token") val bearerToken: String? = null)
