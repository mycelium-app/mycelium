package mycelium.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonObject

val MyceliumJson = Json {
    ignoreUnknownKeys = true
    explicitNulls = false
    encodeDefaults = false
}

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

@Serializable
enum class LoginState {
    @SerialName("signed_in")
    SignedIn,

    @SerialName("signed_out")
    SignedOut,

    @SerialName("expired")
    Expired,

    @SerialName("unknown")
    Unknown,
}

@Serializable
enum class DeviceKind {
    @SerialName("node")
    Node,

    @SerialName("client")
    Client,
}

object EventKinds {
    const val UserMessage = "user_message"
    const val AcpUpdate = "acp_update"
    const val PermissionRequest = "permission_request"
    const val PermissionResolved = "permission_resolved"
    const val TurnEnd = "turn_end"
    const val Error = "error"
    const val Workflow = "workflow"
}

@Serializable
data class ErrorBody(val error: String, val message: String)

@Serializable
data class Me(val id: String, val email: String, val name: String)

@Serializable
data class DeviceRequest(
    val kind: DeviceKind,
    val label: String,
    val hostname: String,
    val os: String,
    val arch: String,
)

@Serializable
data class DeviceResponse(
    @SerialName("device_code") val deviceCode: String,
    @SerialName("user_code") val userCode: String,
    @SerialName("verification_uri") val verificationUri: String,
    @SerialName("verification_uri_complete") val verificationUriComplete: String,
    val interval: Long,
    @SerialName("expires_in") val expiresIn: Long,
)

@Serializable
data class DeviceTokenRequest(@SerialName("device_code") val deviceCode: String)

@Serializable
data class DeviceTokenResponse(val kind: DeviceKind, val id: String, val token: String)

@Serializable
data class PendingDevice(
    @SerialName("user_code") val userCode: String,
    val kind: DeviceKind,
    val label: String,
    val hostname: String,
    val os: String,
    val arch: String,
    val ip: String,
    @SerialName("expires_at") val expiresAt: String,
)

@Serializable
data class AgentInfo(val agent: String, val version: String? = null, val available: Boolean)

@Serializable
data class LoginInfo(
    val agent: String,
    val account: String? = null,
    val method: String,
    val state: LoginState,
)

@Serializable
data class NodeSummary(
    val id: String,
    val name: String,
    val icon: String? = null,
    val hostname: String,
    val os: String,
    val arch: String,
    val version: String,
    val online: Boolean,
    val draining: Boolean,
    @SerialName("last_seen_at") val lastSeenAt: String? = null,
    val agents: List<AgentInfo> = emptyList(),
    val logins: List<LoginInfo> = emptyList(),
)

@Serializable
data class DrainRequest(val draining: Boolean)

@Serializable
data class UpdateNodeRequest(val name: String? = null, val icon: String? = null)

@Serializable
data class Memory(
    val id: String,
    val text: String,
    val scopes: List<String> = emptyList(),
    @SerialName("created_by") val createdBy: String,
    @SerialName("conversation_id") val conversationId: String? = null,
    @SerialName("created_at") val createdAt: String,
    @SerialName("updated_at") val updatedAt: String,
)

@Serializable
data class UpdateMemoryRequest(val text: String)

@Serializable
data class Forge(
    val id: String,
    val kind: String,
    @SerialName("base_url") val baseUrl: String,
    val account: String,
    @SerialName("token_hint") val tokenHint: String,
    val organisations: List<String> = emptyList(),
    @SerialName("created_at") val createdAt: String,
)

@Serializable
data class ConnectForgeRequest(val kind: String, @SerialName("base_url") val baseUrl: String? = null, val token: String)

@Serializable
data class Project(
    val id: String,
    @SerialName("node_id") val nodeId: String,
    val name: String,
    val path: String,
    @SerialName("forge_id") val forgeId: String? = null,
    val repository: String? = null,
    @SerialName("created_at") val createdAt: String,
)

@Serializable
data class CreateProjectRequest(
    @SerialName("node_id") val nodeId: String,
    @SerialName("forge_id") val forgeId: String? = null,
    val repository: String? = null,
    val path: String? = null,
)

@Serializable
data class Worktree(
    val id: String,
    @SerialName("project_id") val projectId: String,
    val branch: String,
    val path: String,
    @SerialName("issue_number") val issueNumber: Long? = null,
    @SerialName("created_at") val createdAt: String,
)
@Serializable
data class Repository(
    @SerialName("forge_id") val forgeId: String,
    val owner: String,
    val name: String,
    @SerialName("full_name") val fullName: String,
    val description: String? = null,
    val private: Boolean = false,
    @SerialName("default_branch") val defaultBranch: String? = null,
)

@Serializable
data class RepositoryOrganisation(@SerialName("forge_id") val forgeId: String, val name: String)

@Serializable
data class RepositoryFailure(@SerialName("forge_id") val forgeId: String, val message: String)

@Serializable
data class Repositories(
    val repositories: List<Repository> = emptyList(),
    val organisations: List<RepositoryOrganisation> = emptyList(),
    val failures: List<RepositoryFailure> = emptyList(),
)

@Serializable
data class StartLoginRequest(val agent: String, val method: String)

@Serializable
data class StartLoginResponse(@SerialName("login_id") val loginId: String)

@Serializable
data class LoginInputRequest(val text: String)

@Serializable
data class StartTerminalRequest(val cols: Int, val rows: Int)

@Serializable
data class StartTerminalResponse(@SerialName("terminal_id") val terminalId: String)

@Serializable
data class TerminalInputRequest(val data: String)

@Serializable
data class ResizeTerminalRequest(val cols: Int, val rows: Int)

@Serializable
data class ImportStatus(@SerialName("node_id") val nodeId: String, val agent: String, val found: Int, val imported: Int)

@Serializable
data class ImportRequest(@SerialName("node_id") val nodeId: String, val agent: String)

@Serializable
data class ImportResult(val imported: Int, val skipped: Int = 0)

@Serializable
data class NodeIndexing(
    @SerialName("node_id") val nodeId: String,
    val transcripts: Int,
    val imported: Int,
    val conversations: Int,
)

@Serializable
data class Indexing(val conversations: Int, val entries: Int, val nodes: List<NodeIndexing> = emptyList())

@Serializable
data class SearchHit(
    @SerialName("conversation_id") val conversationId: String,
    val title: String,
    @SerialName("node_id") val nodeId: String,
    @SerialName("updated_at") val updatedAt: String,
    val snippet: String,
)

@Serializable
data class CreateConversationRequest(
    @SerialName("node_id") val nodeId: String,
    val agent: String,
    val cwd: String? = null,
    @SerialName("project_id") val projectId: String? = null,
    val title: String? = null,
    val prompt: String? = null,
)

@Serializable
data class PatchConversationRequest(val title: String? = null, val archived: Boolean? = null)

@Serializable
data class PromptRequest(val text: String)

@Serializable
data class PatchQueueRequest(val text: String)

@Serializable
data class PermissionAnswer(
    @SerialName("request_id") val requestId: String,
    @SerialName("option_id") val optionId: String,
    val branch: String? = null,
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
)

@Serializable
data class Event(
    @SerialName("conversation_id") val conversationId: String,
    val seq: Long,
    val kind: String,
    val payload: JsonElement = JsonObject(emptyMap()),
)

@Serializable
data class QueuedPrompt(
    val id: String,
    @SerialName("conversation_id") val conversationId: String,
    val position: Int,
    val text: String,
    @SerialName("created_at") val createdAt: String,
)

sealed interface StreamMessage {
    data class ConversationChanged(val conversation: Conversation) : StreamMessage

    data class EventAppended(val event: Event) : StreamMessage

    data class QueueChanged(val conversationId: String, val queue: List<QueuedPrompt>) : StreamMessage

    data class NodeChanged(val node: NodeSummary) : StreamMessage

    data class NodeRemoved(val nodeId: String) : StreamMessage

    data class LoginOutput(val loginId: String, val data: String) : StreamMessage

    data class LoginFinished(val loginId: String, val ok: Boolean) : StreamMessage

    data class TerminalOutput(val terminalId: String, val data: String) : StreamMessage

    data class TerminalExited(val terminalId: String) : StreamMessage
}

fun parseStreamMessage(text: String): StreamMessage? {
    val element = MyceliumJson.parseToJsonElement(text).jsonObject
    val type = (element["type"] as? JsonPrimitive)?.contentOrNull

    return when (type) {
        "conversation" -> StreamMessage.ConversationChanged(MyceliumJson.decodeFromJsonElement(Conversation.serializer(), element))
        "event" -> StreamMessage.EventAppended(MyceliumJson.decodeFromJsonElement(Event.serializer(), element))
        "queue" -> StreamMessage.QueueChanged(
            conversationId = element.requireString("conversation_id"),
            queue = MyceliumJson.decodeFromJsonElement(
                kotlinx.serialization.builtins.ListSerializer(QueuedPrompt.serializer()),
                element.getValue("queue"),
            ),
        )
        "node" -> StreamMessage.NodeChanged(MyceliumJson.decodeFromJsonElement(NodeSummary.serializer(), element))
        "node_removed" -> StreamMessage.NodeRemoved(element.requireString("node_id"))
        "login_output" -> StreamMessage.LoginOutput(element.requireString("login_id"), element.requireString("data"))
        "login_finished" -> StreamMessage.LoginFinished(
            loginId = element.requireString("login_id"),
            ok = (element["ok"] as? JsonPrimitive)?.contentOrNull == "true",
        )
        "terminal_output" -> StreamMessage.TerminalOutput(element.requireString("terminal_id"), element.requireString("data"))
        "terminal_exited" -> StreamMessage.TerminalExited(element.requireString("terminal_id"))
        else -> null
    }
}

private fun JsonObject.requireString(key: String): String =
    (this[key] as? JsonPrimitive)?.contentOrNull ?: error("Stream message is missing $key")

@Serializable
data class GitFile(
    val path: String,
    val staged: Boolean,
    val unstaged: Boolean,
    val untracked: Boolean,
    val additions: Int,
    val deletions: Int,
)

@Serializable
data class GitStatus(val branch: String, val upstream: String? = null, val ahead: Int, val behind: Int, val files: List<GitFile>)

@Serializable
data class GitCommit(val hash: String, val author: String, val date: String, val subject: String)

@Serializable
data class GitDiff(val diff: String, val truncated: Boolean)

@Serializable
data class GitPathsRequest(val paths: List<String>)

@Serializable
data class GitCommitRequest(val message: String)
