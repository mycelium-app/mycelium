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
    const val ModelChanged = "model_changed"
    const val Continued = "continued"
}

@Serializable
data class ErrorBody(val error: String, val message: String)

@Serializable
data class Me(val id: String, val email: String, val name: String)

@Serializable
data class ServerVersion(val version: String)

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
data class AgentInfo(
    val agent: String,
    val version: String? = null,
    val available: Boolean,
    @SerialName("update_available") val updateAvailable: String? = null,
)

@Serializable
data class LoginInfo(
    val agent: String,
    val account: String? = null,
    val method: String,
    val state: LoginState,
    @SerialName("usage_left") val usageLeft: Int? = null,
    val windows: List<UsageWindow> = emptyList(),
    @SerialName("usage_read_at") val usageReadAt: String? = null,
    @SerialName("reset_credits") val resetCredits: ResetCredits? = null,
)

@Serializable
data class ResetCredits(
    @SerialName("available_count") val availableCount: Int,
    val credits: List<ResetCredit>? = null,
)

@Serializable
data class ResetCredit(
    val id: String,
    @SerialName("expires_at") val expiresAt: String? = null,
    val title: String? = null,
)

@Serializable
data class UseResetRequest(
    val agent: String,
    val account: String? = null,
    @SerialName("idempotency_key") val idempotencyKey: String,
)

@Serializable
data class UseResetResponse(val outcome: String)

@Serializable
data class UsageWindow(
    val name: String,
    @SerialName("used_percent") val usedPercent: Double,
    @SerialName("resets_at") val resetsAt: String? = null,
    @SerialName("rate_per_hour") val ratePerHour: Double? = null,
    @SerialName("runs_out_at") val runsOutAt: String? = null,
)

@Serializable
data class NodeSummary(
    val id: String,
    val name: String,
    val icon: String? = null,
    val colour: String? = null,
    val hostname: String,
    val os: String,
    val arch: String,
    val version: String,
    val address: String = "",
    @SerialName("update_available") val updateAvailable: String? = null,
    @SerialName("update_message") val updateMessage: String? = null,
    val online: Boolean,
    val draining: Boolean,
    @SerialName("last_seen_at") val lastSeenAt: String? = null,
    val agents: List<AgentInfo> = emptyList(),
    val logins: List<LoginInfo> = emptyList(),
    val sessions: List<String> = emptyList(),
    @SerialName("instructions_revision") val instructionsRevision: String? = null,
    @SerialName("instructions_unmanaged") val instructionsUnmanaged: List<String> = emptyList(),
    @SerialName("sessions_on_older_instructions") val sessionsOnOlderInstructions: Int = 0,
)

@Serializable
data class DrainRequest(val draining: Boolean)

@Serializable
data class UpdateNodeRequest(val name: String? = null, val icon: String? = null, val colour: String? = null)

@Serializable
data class DiskUsage(
    val mount: String,
    @SerialName("total_bytes") val totalBytes: Long,
    @SerialName("free_bytes") val freeBytes: Long,
)

@Serializable
data class ProcessUsage(
    val pid: Int,
    val name: String,
    val command: String = "",
    @SerialName("cpu_percent") val cpuPercent: Double,
    @SerialName("memory_bytes") val memoryBytes: Long,
    val agent: Boolean = false,
    val stoppable: Boolean = false,
    @SerialName("conversation_id") val conversationId: String? = null,
    val cwd: String? = null,
)

@Serializable
data class DockerContainer(val name: String, val state: String)

@Serializable
data class DockerUsage(
    val running: Int = 0,
    val containers: List<DockerContainer> = emptyList(),
    @SerialName("reclaimable_bytes") val reclaimableBytes: Long? = null,
)

@Serializable
data class ListeningPort(
    val port: Int,
    val address: String,
    val pid: Int? = null,
    val process: String? = null,
    @SerialName("conversation_id") val conversationId: String? = null,
    val cwd: String? = null,
)

@Serializable
data class ResourceSample(
    @SerialName("at_ms") val atMs: Long,
    val available: Boolean,
    @SerialName("cpu_percent") val cpuPercent: Double = 0.0,
    val cores: Int = 0,
    val load: List<Double> = emptyList(),
    @SerialName("memory_total_bytes") val memoryTotalBytes: Long = 0,
    @SerialName("memory_available_bytes") val memoryAvailableBytes: Long = 0,
    @SerialName("swap_total_bytes") val swapTotalBytes: Long = 0,
    @SerialName("swap_free_bytes") val swapFreeBytes: Long = 0,
    val disks: List<DiskUsage> = emptyList(),
    val processes: List<ProcessUsage> = emptyList(),
    val ports: List<ListeningPort> = emptyList(),
    val docker: DockerUsage? = null,
)

@Serializable
data class CpuPoint(@SerialName("at_ms") val atMs: Long, @SerialName("cpu_percent") val cpuPercent: Double)

@Serializable
data class NodeResources(val latest: ResourceSample? = null, val history: List<CpuPoint> = emptyList())

@Serializable
data class ConversationUsage(
    @SerialName("context_used") val contextUsed: Long? = null,
    @SerialName("context_size") val contextSize: Long? = null,
    @SerialName("input_tokens") val inputTokens: Long = 0,
    @SerialName("output_tokens") val outputTokens: Long = 0,
    @SerialName("cache_read_tokens") val cacheReadTokens: Long = 0,
    @SerialName("cache_write_tokens") val cacheWriteTokens: Long = 0,
    @SerialName("cost_usd") val costUsd: Double? = null,
    @SerialName("cost_estimated") val costEstimated: Boolean = false,
) {
    val tokens: Long get() = inputTokens + outputTokens + cacheReadTokens + cacheWriteTokens
}

@Serializable
data class ActivityRow(
    val key: String,
    val name: String,
    val turns: Long = 0,
    @SerialName("reported_turns") val reportedTurns: Long = 0,
    @SerialName("working_ms") val workingMs: Long = 0,
    @SerialName("input_tokens") val inputTokens: Long = 0,
    @SerialName("output_tokens") val outputTokens: Long = 0,
    @SerialName("cache_read_tokens") val cacheReadTokens: Long = 0,
    @SerialName("cache_write_tokens") val cacheWriteTokens: Long = 0,
    @SerialName("subagent_tokens") val subagentTokens: Long = 0,
    @SerialName("cost_usd") val costUsd: Double? = null,
    @SerialName("cost_estimated") val costEstimated: Boolean = false,
) {
    val tokens: Long get() = inputTokens + outputTokens + cacheReadTokens + cacheWriteTokens
}

@Serializable
data class Activity(
    val range: String,
    val totals: ActivityRow,
    val days: List<ActivityRow> = emptyList(),
    val hours: List<ActivityRow> = emptyList(),
    val conversations: List<ActivityRow> = emptyList(),
    val nodes: List<ActivityRow> = emptyList(),
    val agents: List<ActivityRow> = emptyList(),
    val projects: List<ActivityRow> = emptyList(),
    val models: List<ActivityRow> = emptyList(),
    @SerialName("unreported_agents") val unreportedAgents: List<String> = emptyList(),
)

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
data class InstructionProposal(
    val id: String,
    @SerialName("conversation_id") val conversationId: String? = null,
    @SerialName("conversation_title") val conversationTitle: String? = null,
    val layer: String,
    @SerialName("layer_name") val layerName: String,
    @SerialName("base_revision") val baseRevision: String,
    @SerialName("base_text") val baseText: String,
    val text: String,
    val reason: String,
    val state: String,
    val note: String? = null,
    @SerialName("out_of_date") val outOfDate: Boolean = false,
    @SerialName("created_at") val createdAt: String,
    @SerialName("decided_at") val decidedAt: String? = null,
) {
    val waiting: Boolean get() = state == "proposed"
}

@Serializable
data class DeclineInstructionProposalRequest(val note: String? = null, val redo: Boolean = false)

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
data class ModelProvider(
    val id: String,
    val kind: String,
    val name: String,
    @SerialName("base_url") val baseUrl: String? = null,
    @SerialName("key_hint") val keyHint: String? = null,
    val models: List<String> = emptyList(),
    @SerialName("default_model") val defaultModel: String? = null,
    @SerialName("created_at") val createdAt: String,
    val favourites: List<String> = emptyList(),
    @SerialName("model_class") val modelClass: String = "cloud",
    @SerialName("speech_model") val speechModel: String? = null,
)

@Serializable
data class Transcription(val text: String)

@Serializable
data class ModelProviderUsage(
    @SerialName("provider_id") val providerId: String,
    val name: String,
    @SerialName("read_at") val readAt: String? = null,
    val spend: OpenRouterSpend? = null,
)

@Serializable
data class OpenRouterSpend(
    val total: Double,
    val daily: Double? = null,
    val weekly: Double? = null,
    val monthly: Double? = null,
    @SerialName("limit_remaining") val limitRemaining: Double? = null,
    val limit: Double? = null,
    @SerialName("limit_reset") val limitReset: String? = null,
)

@Serializable
data class CreateModelProviderRequest(
    val kind: String,
    val name: String,
    @SerialName("base_url") val baseUrl: String? = null,
    @SerialName("api_key") val apiKey: String? = null,
)

@Serializable
data class UpdateModelProviderRequest(val name: String? = null, @SerialName("model_class") val modelClass: String? = null)

@Serializable
data class SetFavouriteModelRequest(val model: String, val favourite: Boolean)

@Serializable
data class ChatToolInfo(val name: String, val description: String = "")

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
    @SerialName("always_allow") val alwaysAllow: Boolean = false,
    val tools: List<ChatToolInfo> = emptyList(),
    @SerialName("created_at") val createdAt: String,
)

@Serializable
data class CreateChatToolServerRequest(val name: String, val url: String, @SerialName("bearer_token") val bearerToken: String? = null, val kind: String? = null, val account: String? = null, val settings: JsonObject? = null)

@Serializable
data class StartChatToolServerOAuthRequest(val name: String, val url: String, val kind: String? = null)

@Serializable
data class OAuthStart(@SerialName("authorize_url") val authorizeUrl: String)

@Serializable
data class PatchChatToolServerRequest(val enabled: Boolean? = null, @SerialName("always_allow") val alwaysAllow: Boolean? = null, val name: String? = null)

@Serializable
data class CreateChatRequest(
    @SerialName("provider_id") val providerId: String,
    val model: String,
    val prompt: String? = null,
    @SerialName("parent_id") val parentId: String? = null,
)

@Serializable
data class DirectoryEntry(val name: String, val path: String, val repository: Boolean = false)

@Serializable
data class Directories(val root: String, val path: String, val parent: String? = null, val directories: List<DirectoryEntry> = emptyList(), val truncated: Boolean = false)

@Serializable
data class Project(
    val id: String,
    @SerialName("node_id") val nodeId: String,
    val name: String,
    val path: String,
    @SerialName("forge_id") val forgeId: String? = null,
    val repository: String? = null,
    @SerialName("created_at") val createdAt: String,
    @SerialName("group_id") val groupId: String? = null,
    val icon: String? = null,
    @SerialName("avatar_sha256") val avatarSha256: String? = null,
)

@Serializable
data class PatchProjectRequest(
    @SerialName("group_id") val groupId: String? = null,
    val icon: String? = null,
    @SerialName("clear_icon") val clearIcon: Boolean = false,
)

@Serializable
data class Group(val id: String, val name: String, val folders: List<GroupFolder> = emptyList(), @SerialName("created_at") val createdAt: String)

@Serializable
data class ModelChoice(
    val agent: String? = null,
    @SerialName("provider_id") val providerId: String? = null,
    val model: String? = null,
    val account: String? = null,
)

@Serializable
data class AssistantPreferences(
    val models: List<ModelChoice> = emptyList(),
    val nodes: List<String> = emptyList(),
    @SerialName("chat_first") val chatFirst: Boolean = true,
    @SerialName("fall_back_on") val fallBackOn: List<String> = listOf("usage_limit", "signed_out", "offline"),
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

@Serializable
data class GroupFolder(val id: String, @SerialName("node_id") val nodeId: String? = null, val prefix: String)

@Serializable
data class GroupFolderRequest(val prefix: String, @SerialName("node_id") val nodeId: String? = null)

@Serializable
data class GroupRequest(val name: String)

@Serializable
data class Extensions(
    @SerialName("generated_ui") val generatedUi: Boolean,
    @SerialName("rich_markdown") val richMarkdown: Boolean,
    @SerialName("mycelium_tools") val myceliumTools: Boolean,
)

@Serializable
data class Experiments(val voice: Boolean = false)

@Serializable
data class UsageLimitSetting(@SerialName("on_usage_limit") val onUsageLimit: String)

@Serializable
data class InstructionVersion(val id: String, @SerialName("created_at") val createdAt: String)

@Serializable
data class InstructionLayer(
    val layer: String,
    val name: String,
    val path: String? = null,
    val text: String = "",
    val version: InstructionVersion? = null,
)

@Serializable
data class InstructionSource(
    @SerialName("forge_id") val forgeId: String,
    val repository: String,
    val path: String,
)

@Serializable
data class Instructions(
    val source: InstructionSource? = null,
    val revision: String? = null,
    val layers: List<InstructionLayer> = emptyList(),
)

@Serializable
data class SaveInstructionsRequest(val text: String)

@Serializable
data class SetInstructionSourceRequest(
    @SerialName("forge_id") val forgeId: String,
    val repository: String,
    val path: String? = null,
)

@Serializable
data class CreateProjectRequest(
    @SerialName("node_id") val nodeId: String,
    @SerialName("forge_id") val forgeId: String? = null,
    val repository: String? = null,
    val path: String? = null,
    @SerialName("new_repository") val newRepository: Boolean = false,
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

@Serializable
data class Schedule(
    val id: String,
    @SerialName("node_id") val nodeId: String,
    val agent: String,
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
    @SerialName("created_at") val createdAt: String,
)

@Serializable
data class CreateScheduleRequest(
    @SerialName("node_id") val nodeId: String,
    val agent: String,
    val prompt: String,
    val cron: String,
    @SerialName("each_run") val eachRun: String,
    val cwd: String? = null,
    @SerialName("project_id") val projectId: String? = null,
    @SerialName("time_zone") val timeZone: String? = null,
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

@Serializable
data class Artifact(
    val id: String,
    val owner: String,
    @SerialName("conversation_id") val conversationId: String? = null,
    @SerialName("project_ids") val projectIds: List<String>? = null,
    @SerialName("conversation_ids") val conversationIds: List<String>? = null,
    val kind: String,
    val title: String,
    val slug: String = "",
    @SerialName("public") val isPublic: Boolean = false,
    @SerialName("latest_version") val latestVersion: Int = 1,
    @SerialName("thumbnail_version") val thumbnailVersion: Int? = null,
    @SerialName("media_type") val mediaType: String? = null,
    val url: String? = null,
    @SerialName("created_at") val createdAt: String,
    @SerialName("updated_at") val updatedAt: String,
)

@Serializable
data class ArtifactLink(
    val id: String,
    @SerialName("artifact_id") val artifactId: String,
    @SerialName("project_id") val projectId: String? = null,
    @SerialName("conversation_id") val conversationId: String? = null,
    val permission: String,
)

@Serializable
data class LinkArtifactRequest(
    @SerialName("project_id") val projectId: String? = null,
    @SerialName("conversation_id") val conversationId: String? = null,
    val permission: String,
)

@Serializable
data class ArtifactAccessRequest(
    val id: String,
    @SerialName("artifact_id") val artifactId: String,
    @SerialName("conversation_id") val conversationId: String,
    val permission: String,
    val reason: String,
    val state: String,
    @SerialName("created_at") val createdAt: String,
    @SerialName("decided_at") val decidedAt: String? = null,
)

@Serializable
data class ApproveArtifactAccessRequest(val project: Boolean = false)

@Serializable
data class PatchArtifactRequest(
    val title: String? = null,
    @SerialName("public") val isPublic: Boolean? = null,
)

object ArtifactKinds {
    const val Design = "design"
    const val Page = "page"
    const val Markdown = "markdown"
    const val File = "file"
}

object EachRun {
    const val New = "new"
    const val Same = "same"
}

@Serializable
data class Issue(
    @SerialName("forge_id") val forgeId: String,
    val repository: String,
    val number: Long,
    val title: String,
    val url: String,
    val body: String? = null,
    @SerialName("pull_requests") val pullRequests: List<PullRequest>? = null,
)

@Serializable
enum class PullRequestState {
    @SerialName("open") Open,
    @SerialName("merged") Merged,
    @SerialName("closed") Closed,
}

@Serializable
data class PullRequest(val number: Long, val label: String, val url: String, val state: PullRequestState)

@Serializable
data class Issues(val issues: List<Issue> = emptyList(), val failures: List<RepositoryFailure> = emptyList())

@Serializable
data class StartWorkspaceRequest(val agent: String)

@Serializable
data class Repository(
    @SerialName("forge_id") val forgeId: String,
    val owner: String,
    val name: String,
    @SerialName("full_name") val fullName: String,
    val description: String? = null,
    val private: Boolean = false,
    @SerialName("default_branch") val defaultBranch: String? = null,
    @SerialName("updated_at") val updatedAt: String? = null,
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
data class StartLoginRequest(val agent: String, val method: String, val account: String? = null)

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
data class ImportProblem(val path: String, val reason: String)

@Serializable
data class ImportJob(
    val id: String,
    @SerialName("node_id") val nodeId: String,
    val agent: String,
    val state: String,
    val total: Int,
    val done: Int = 0,
    val imported: Int = 0,
    val skipped: Int = 0,
    val waiting: Int = 0,
    val failed: List<ImportProblem> = emptyList(),
) {
    val running: Boolean get() = state == "running"
}

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
    val account: String? = null,
    val cwd: String? = null,
    @SerialName("project_id") val projectId: String? = null,
    val branch: String? = null,
    val title: String? = null,
    val prompt: String? = null,
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
data class PromptRequest(val text: String, val attachments: List<Attachment> = emptyList())

@Serializable
data class PromptAccepted(val queued: Boolean, val message: String)

@Serializable
data class PatchQueueRequest(val text: String)

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
enum class GoalState {
    @SerialName("active")
    Active,

    @SerialName("paused")
    Paused,

    @SerialName("met")
    Met,

    @SerialName("stuck")
    Stuck,

    @SerialName("over_budget")
    OverBudget,

    @SerialName("limit_reached")
    LimitReached,
}

@Serializable
data class Goal(
    @SerialName("conversation_id") val conversationId: String,
    val text: String,
    @SerialName("token_budget") val tokenBudget: Long? = null,
    @SerialName("tokens_used") val tokensUsed: Long = 0,
    val turns: Long = 0,
    val state: GoalState,
    val reason: String? = null,
    @SerialName("time_limit_seconds") val timeLimitSeconds: Long? = null,
    @SerialName("five_hour_limit") val fiveHourLimit: Int? = null,
    @SerialName("weekly_limit") val weeklyLimit: Int? = null,
    @SerialName("started_at") val startedAt: String? = null,
    @SerialName("five_hour_used") val fiveHourUsed: Double? = null,
    @SerialName("weekly_used") val weeklyUsed: Double? = null,
    @SerialName("tokens_known") val tokensKnown: Boolean = true,
    @SerialName("turn_active") val turnActive: Boolean = false,
)

@Serializable
data class SetGoalRequest(
    val text: String,
    @SerialName("token_budget") val tokenBudget: Long? = null,
    @SerialName("time_limit_seconds") val timeLimitSeconds: Long? = null,
    @SerialName("five_hour_limit") val fiveHourLimit: Int? = null,
    @SerialName("weekly_limit") val weeklyLimit: Int? = null,
)

@Serializable
data class PatchGoalRequest(val paused: Boolean)

@Serializable
data class WalkthroughFile(val path: String, val diff: String, val unchanged: Boolean = false)

@Serializable
data class WalkthroughStep(val title: String, val key: Boolean = false, val section: String? = null, val icon: String? = null, val explanation: String = "", val files: List<WalkthroughFile> = emptyList())

@Serializable
data class WalkthroughNote(
    val id: String,
    val step: Int,
    val file: String? = null,
    @SerialName("line_start") val lineStart: Int? = null,
    @SerialName("line_end") val lineEnd: Int? = null,
    val quote: String = "",
    val comment: String = "",
    val sent: Boolean = false,
    @SerialName("created_at") val createdAt: String = "",
)

@Serializable
data class WalkthroughNoteRequest(
    val step: Int,
    val file: String? = null,
    @SerialName("line_start") val lineStart: Int? = null,
    @SerialName("line_end") val lineEnd: Int? = null,
    val quote: String = "",
    val comment: String = "",
)

@Serializable
data class Walkthrough(
    val id: String,
    @SerialName("conversation_id") val conversationId: String,
    val steps: List<WalkthroughStep>,
    @SerialName("created_at") val createdAt: String,
    val dismissed: Boolean = false,
    val scope: String = "branch",
    val note: String? = null,
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

sealed interface StreamMessage {
    data class ConversationChanged(val conversation: Conversation) : StreamMessage

    data class EventAppended(val event: Event) : StreamMessage

    data class ConversationRemoved(val conversationId: String) : StreamMessage

    data class QueueChanged(val conversationId: String, val queue: List<QueuedPrompt>) : StreamMessage

    data class MoveChanged(val progress: MoveProgress) : StreamMessage

    data class GoalChanged(val goal: Goal) : StreamMessage

    data class GoalCleared(val conversationId: String) : StreamMessage

    data class WalkthroughChanged(val walkthrough: Walkthrough) : StreamMessage

    data class WalkthroughNotesChanged(val conversationId: String, val notes: List<WalkthroughNote>) : StreamMessage

    data class WorkflowChanged(val workflow: JsonObject) : StreamMessage

    data class ScheduleChanged(val schedule: Schedule) : StreamMessage

    data class AssistantChanged(val assistant: Assistant) : StreamMessage

    data class ScheduleRemoved(val scheduleId: String) : StreamMessage

    data class ArtifactChanged(val artifact: Artifact, val deleted: Boolean = false) : StreamMessage

    data class ArtifactCommentAdded(val artifactId: String) : StreamMessage
    data class ArtifactAccessRequestChanged(val request: ArtifactAccessRequest) : StreamMessage

    data class InstructionProposalChanged(val proposal: InstructionProposal) : StreamMessage

    data class MessageProposalChanged(val proposal: MessageProposal) : StreamMessage

    data class ConversationConnectionChanged(val connection: ConversationConnection) : StreamMessage

    data class GroupConnectionChanged(val connection: GroupConnection) : StreamMessage

    data class ApiAccessRequestChanged(val request: ApiAccessRequest) : StreamMessage

    data class SshAccessRequestChanged(val request: SshAccessRequest) : StreamMessage

    data class MemoryChanged(val memory: Memory) : StreamMessage

    data class MemoryRemoved(val memoryId: String) : StreamMessage

    data class MemoryBankChanged(val bank: MemoryBank) : StreamMessage

    data class MemoryBankRemoved(val bankId: String) : StreamMessage

    data class NodeChanged(val node: NodeSummary) : StreamMessage

    data class NodeRemoved(val nodeId: String) : StreamMessage

    data object IntegrationsChanged : StreamMessage

    data class NodeResourcesSampled(val nodeId: String, val sample: ResourceSample) : StreamMessage

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
        "conversation_removed" -> StreamMessage.ConversationRemoved(element.requireString("conversation_id"))
        "event" -> StreamMessage.EventAppended(MyceliumJson.decodeFromJsonElement(Event.serializer(), element))
        "queue" -> StreamMessage.QueueChanged(
            conversationId = element.requireString("conversation_id"),
            queue = MyceliumJson.decodeFromJsonElement(
                kotlinx.serialization.builtins.ListSerializer(QueuedPrompt.serializer()),
                element.getValue("queue"),
            ),
        )
        "move" -> StreamMessage.MoveChanged(MyceliumJson.decodeFromJsonElement(MoveProgress.serializer(), element))
        "goal" -> StreamMessage.GoalChanged(MyceliumJson.decodeFromJsonElement(Goal.serializer(), element))
        "goal_cleared" -> StreamMessage.GoalCleared(element.requireString("conversation_id"))
        "walkthrough" -> StreamMessage.WalkthroughChanged(MyceliumJson.decodeFromJsonElement(Walkthrough.serializer(), element))
        "walkthrough_notes" -> StreamMessage.WalkthroughNotesChanged(
            conversationId = element.requireString("conversation_id"),
            notes = MyceliumJson.decodeFromJsonElement(
                kotlinx.serialization.builtins.ListSerializer(WalkthroughNote.serializer()),
                element.getValue("notes"),
            ),
        )
        "workflow" -> StreamMessage.WorkflowChanged(element["workflow"]!!.jsonObject)
        "assistant" -> StreamMessage.AssistantChanged(MyceliumJson.decodeFromJsonElement(Assistant.serializer(), element))
        "schedule" -> StreamMessage.ScheduleChanged(MyceliumJson.decodeFromJsonElement(Schedule.serializer(), element))
        "schedule_removed" -> StreamMessage.ScheduleRemoved(element.requireString("schedule_id"))
        "artifact" -> StreamMessage.ArtifactChanged(
            MyceliumJson.decodeFromJsonElement(Artifact.serializer(), element),
            deleted = (element["deleted"] as? JsonPrimitive)?.contentOrNull == "true",
        )
        "artifact_access_request" -> StreamMessage.ArtifactAccessRequestChanged(MyceliumJson.decodeFromJsonElement(ArtifactAccessRequest.serializer(), element))
        "artifact_comment" -> StreamMessage.ArtifactCommentAdded(element.requireString("artifact_id"))
        "instruction_proposal" -> StreamMessage.InstructionProposalChanged(MyceliumJson.decodeFromJsonElement(InstructionProposal.serializer(), element))
        "conversation_connection" -> StreamMessage.ConversationConnectionChanged(MyceliumJson.decodeFromJsonElement(ConversationConnection.serializer(), element))
        "group_connection" -> StreamMessage.GroupConnectionChanged(MyceliumJson.decodeFromJsonElement(GroupConnection.serializer(), element))
        "message_proposal" -> StreamMessage.MessageProposalChanged(MyceliumJson.decodeFromJsonElement(MessageProposal.serializer(), element))
        "api_access_request" -> StreamMessage.ApiAccessRequestChanged(MyceliumJson.decodeFromJsonElement(ApiAccessRequest.serializer(), element))
        "ssh_access_request" -> StreamMessage.SshAccessRequestChanged(MyceliumJson.decodeFromJsonElement(SshAccessRequest.serializer(), element))
        "memory" -> StreamMessage.MemoryChanged(MyceliumJson.decodeFromJsonElement(Memory.serializer(), element))
        "memory_removed" -> StreamMessage.MemoryRemoved(element.requireString("memory_id"))
        "memory_bank" -> StreamMessage.MemoryBankChanged(MyceliumJson.decodeFromJsonElement(MemoryBank.serializer(), element))
        "memory_bank_removed" -> StreamMessage.MemoryBankRemoved(element.requireString("bank_id"))
        "node" -> StreamMessage.NodeChanged(MyceliumJson.decodeFromJsonElement(NodeSummary.serializer(), element))
        "node_removed" -> StreamMessage.NodeRemoved(element.requireString("node_id"))
        "integrations_changed" -> StreamMessage.IntegrationsChanged
        "node_resources" -> StreamMessage.NodeResourcesSampled(
            element.requireString("node_id"),
            MyceliumJson.decodeFromJsonElement(ResourceSample.serializer(), element.getValue("sample")),
        )
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
data class GitStatus(
    val branch: String,
    val upstream: String? = null,
    val ahead: Int,
    val behind: Int,
    val files: List<GitFile>,
    @SerialName("prepared_message") val preparedMessage: String? = null,
)

@Serializable
data class GitUndone(val message: String = "")

@Serializable
data class GitCommit(val hash: String, val author: String, val date: String, val subject: String)

@Serializable
data class GitDiff(val diff: String, val truncated: Boolean)

@Serializable
data class ChangedFile(
    val path: String,
    val staged: Boolean,
    val unstaged: Boolean,
    val untracked: Boolean,
    val added: Int,
    val removed: Int,
    val rows: Int,
    val hunks: List<Int> = emptyList(),
    val truncated: Boolean = false,
    val fingerprint: String,
    @SerialName("staged_rows") val stagedRows: Int = 0,
)

@Serializable
data class ChangesLayout(val files: List<ChangedFile>)

@Serializable
data class RowRange(val path: String, val from: Int, val to: Int)

@Serializable
data class RowsRequest(val ranges: List<RowRange>)

@Serializable
data class WireRow(val k: String, val o: Int? = null, val n: Int? = null, val t: String)

@Serializable
data class RowsChunk(val path: String, val from: Int, val fingerprint: String, val rows: List<WireRow>)

@Serializable
data class ChangesRows(val ranges: List<RowsChunk>)

@Serializable
data class GitHunkRequest(val path: String, val row: Int, val fingerprint: String, val action: String)

@Serializable
data class GitFileRequest(val path: String, val content: String? = null, val expected: String? = null)

@Serializable
data class GitFileContent(val content: String)

@Serializable
data class GitPathsRequest(val paths: List<String>)

@Serializable
data class GitPullRequest(val rebase: Boolean = false)

@Serializable
data class GitPushRequest(val force: Boolean = false)

@Serializable
data class GitCommitRequest(
    val message: String,
    val amend: Boolean = false,
    val signoff: Boolean = false,
    @SerialName("no_verify") val noVerify: Boolean = false,
)
