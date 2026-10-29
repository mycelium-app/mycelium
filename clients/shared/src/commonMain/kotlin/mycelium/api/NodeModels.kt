package mycelium.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

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
    @SerialName("harness_models") val harnessModels: List<HarnessModels> = emptyList(),
    val environment: EnvironmentCapability? = null,
    val kubernetes: KubernetesNode? = null,
)

@Serializable
enum class PodState {
    @SerialName("pending")
    Pending,

    @SerialName("running")
    Running,

    @SerialName("exited")
    Exited,
}

@Serializable
data class SessionPod(
    @SerialName("conversation_id") val conversationId: String,
    val name: String,
    val state: PodState,
)

@Serializable
data class KubernetesNode(
    val namespace: String,
    val image: String,
    @SerialName("credentials_secret") val credentialsSecret: String,
    @SerialName("workspace_persistent") val workspacePersistent: Boolean = false,
    val pods: List<SessionPod> = emptyList(),
)

@Serializable
data class HarnessModel(val id: String, val name: String)

@Serializable
data class HarnessModels(
    val agent: String,
    val account: String? = null,
    val models: List<HarnessModel> = emptyList(),
    @SerialName("default_model") val defaultModel: String? = null,
    @SerialName("reported_at") val reportedAt: String,
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
data class OpenTerminal(@SerialName("terminal_id") val terminalId: String, val scrollback: String)

@Serializable
data class TerminalInputRequest(val data: String)

@Serializable
data class ResizeTerminalRequest(val cols: Int, val rows: Int)
