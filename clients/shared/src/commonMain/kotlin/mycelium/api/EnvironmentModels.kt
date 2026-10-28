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
data class EnvironmentSize(val width: Int, val height: Int)

@Serializable
data class EnvironmentTarget(val x: Int, val y: Int, val label: String)

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
)

@Serializable
data class StartEnvironmentRequest(val width: Int? = null, val height: Int? = null)
