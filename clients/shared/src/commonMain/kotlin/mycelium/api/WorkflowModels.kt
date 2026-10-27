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
)

@Serializable
data class WorkflowControlRequest(val action: String, val step: String? = null, val output: String? = null)
