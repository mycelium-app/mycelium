package mycelium.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
data class WorkflowStep(
    val key: String,
    val title: String,
    val prompt: String = "",
    val gate: Boolean = false,
    @SerialName("depends_on") val dependsOn: List<String> = emptyList(),
    val workflow: WorkflowPlan? = null,
    @SerialName("node_id") val nodeId: String? = null,
    val agent: String? = null,
    @SerialName("provider_id") val providerId: String? = null,
    val cwd: String? = null,
    @SerialName("project_id") val projectId: String? = null,
    val branch: String? = null,
    val account: String? = null,
    val model: String? = null,
    val difficulty: String? = null,
)

@Serializable
data class WorkflowPlan(
    val title: String = "",
    @SerialName("max_rounds") val maxRounds: Int = 1,
    @SerialName("stop_when") val stopWhen: String? = null,
    val steps: List<WorkflowStep> = emptyList(),
)

@Serializable
data class WorkflowRunStep(
    val key: String,
    val status: String,
    @SerialName("conversation_id") val conversationId: String? = null,
    @SerialName("workflow_id") val workflowId: String? = null,
    val output: String = "",
    @SerialName("started_at") val startedAt: String? = null,
    @SerialName("ended_at") val endedAt: String? = null,
    val tokens: Long? = null,
    val summary: String = "",
    val error: String? = null,
    val label: String? = null,
    val phase: String? = null,
    @SerialName("agent_id") val agentId: String? = null,
    val turns: Int? = null,
    @SerialName("elapsed_ms") val elapsedMs: Long? = null,
)

@Serializable
data class WorkflowActor(val kind: String, val name: String)

@Serializable
data class WorkflowEndReason(
    val reason: String,
    val step: String? = null,
    val error: String? = null,
    val by: WorkflowActor? = null,
    val round: Int? = null,
)

@Serializable
data class WorkflowDecision(
    val round: Int,
    val step: String,
    val decision: String,
    val by: WorkflowActor,
    val at: String,
    val reason: String? = null,
)

@Serializable
data class WorkflowRound(
    val round: Int,
    @SerialName("started_at") val startedAt: String,
    @SerialName("ended_at") val endedAt: String? = null,
    val reason: String? = null,
    @SerialName("decided_by") val decidedBy: WorkflowActor? = null,
    val steps: List<WorkflowRunStep> = emptyList(),
)

@Serializable
data class Workflow(
    val id: String,
    @SerialName("parent_id") val parentId: String? = null,
    @SerialName("conversation_id") val conversationId: String? = null,
    val title: String,
    val status: String,
    val plan: WorkflowPlan,
    val steps: List<WorkflowRunStep>,
    val round: Int,
    val output: String = "",
    val kind: String = WorkflowKinds.Mycelium,
    @SerialName("created_at") val createdAt: String = "",
    @SerialName("updated_at") val updatedAt: String = "",
    @SerialName("ended_at") val endedAt: String? = null,
    @SerialName("started_by") val startedBy: WorkflowActor? = null,
    @SerialName("project_id") val projectId: String? = null,
    @SerialName("end_reason") val endReason: WorkflowEndReason? = null,
    @SerialName("stop_rule") val stopRule: String? = null,
    @SerialName("rerun_of") val rerunOf: String? = null,
    val rounds: List<WorkflowRound> = emptyList(),
    val decisions: List<WorkflowDecision> = emptyList(),
    val phases: List<String> = emptyList(),
    @SerialName("run_id") val runId: String? = null,
)

object WorkflowKinds {
    const val Mycelium = "mycelium"
    const val AgentRun = "agent_run"
}

object WorkflowStatuses {
    const val Running = "running"
    const val Waiting = "waiting"
    const val Paused = "paused"
    const val Done = "done"
    const val Failed = "failed"
    const val Stopped = "stopped"
}

@Serializable
data class WorkflowControlRequest(val action: String, val step: String? = null, val output: String? = null, val reason: String? = null)
