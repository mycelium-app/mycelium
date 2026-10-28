package mycelium.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
enum class EnvironmentState {
    @SerialName("starting")
    Starting,

    @SerialName("running")
    Running,

    @SerialName("stopped")
    Stopped,

    @SerialName("failed")
    Failed,
}

@Serializable
enum class EnvironmentController {
    @SerialName("agent")
    Agent,

    @SerialName("person")
    Person,
}

@Serializable
enum class EnvironmentAppKind {
    @SerialName("browser")
    Browser,

    @SerialName("file_manager")
    FileManager,

    @SerialName("editor")
    Editor,

    @SerialName("office")
    Office,

    @SerialName("mail")
    Mail,

    @SerialName("terminal")
    Terminal,

    @SerialName("other")
    Other,
}

@Serializable
data class EnvironmentApp(val id: String, val name: String, val kind: EnvironmentAppKind)

@Serializable
data class EnvironmentWindow(
    val id: Long,
    @SerialName("app_id") val appId: String,
    val title: String,
    val focused: Boolean = false,
)

@Serializable
data class EnvironmentSize(val width: Int, val height: Int)

@Serializable
data class EnvironmentTarget(val x: Int, val y: Int, val label: String)

@Serializable
enum class BrowserTaskState {
    @SerialName("running")
    Running,

    @SerialName("done")
    Done,

    @SerialName("blocked")
    Blocked,

    @SerialName("stopped")
    Stopped,

    @SerialName("failed")
    Failed,
}

@Serializable
data class BrowserTaskStep(val label: String, val millis: Long)

@Serializable
data class BrowserTask(
    val goal: String,
    val state: BrowserTaskState,
    val provider: String,
    val steps: List<BrowserTaskStep> = emptyList(),
    val result: String? = null,
    val why: String? = null,
    @SerialName("stopped_by") val stoppedBy: String? = null,
)

@Serializable
data class Environment(
    @SerialName("conversation_id") val conversationId: String,
    val state: EnvironmentState,
    val controller: EnvironmentController,
    @SerialName("node_id") val nodeId: String,
    @SerialName("node_name") val nodeName: String,
    val size: EnvironmentSize,
    @SerialName("started_at") val startedAt: String,
    @SerialName("stopped_at") val stoppedAt: String? = null,
    val failure: String? = null,
    val browser: String? = null,
    val target: EnvironmentTarget? = null,
    @SerialName("browser_task") val browserTask: BrowserTask? = null,
    val windows: List<EnvironmentWindow> = emptyList(),
    @SerialName("agent_window") val agentWindow: Long? = null,
    val person: String? = null,
    @SerialName("closed_with_conversation") val closedWithConversation: Boolean = false,
)

@Serializable
data class EnvironmentAction(val at: String, val label: String)

@Serializable
data class EnvironmentVersions(
    val compositor: String? = null,
    val wayvnc: String? = null,
    val browser: String? = null,
)

@Serializable
data class RunningEnvironment(
    @SerialName("conversation_id") val conversationId: String,
    val controller: EnvironmentController,
    @SerialName("started_at") val startedAt: String,
    @SerialName("memory_bytes") val memoryBytes: Long? = null,
)

@Serializable
data class EnvironmentCapability(
    val available: Boolean,
    val missing: List<String> = emptyList(),
    val versions: EnvironmentVersions = EnvironmentVersions(),
    val note: String? = null,
    val running: List<RunningEnvironment> = emptyList(),
    val apps: List<EnvironmentApp> = emptyList(),
)

@Serializable
data class StartEnvironmentRequest(val width: Int? = null, val height: Int? = null, val apps: List<String>? = null, @SerialName("node_id") val nodeId: String? = null)

@Serializable
data class LastEnvironmentNode(@SerialName("node_id") val nodeId: String? = null)

@Serializable
data class OpenEnvironmentAppRequest(val app: String)
