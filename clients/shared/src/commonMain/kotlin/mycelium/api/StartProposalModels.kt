package mycelium.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
data class StartProposalNode(val id: String, val name: String, val online: Boolean)

@Serializable
data class StartProposalStep(
    val key: String,
    val title: String,
    @SerialName("depends_on") val dependsOn: List<String> = emptyList(),
    val gate: Boolean = false,
    val node: StartProposalNode? = null,
    val folder: String? = null,
    val agent: String? = null,
    val account: String? = null,
    val model: String? = null,
    @SerialName("first_message") val firstMessage: String = "",
    @SerialName("node_choices") val nodeChoices: List<StartProposalNode> = emptyList(),
    @SerialName("account_choices") val accountChoices: List<String> = emptyList(),
)

@Serializable
data class StartProposal(
    val id: String,
    @SerialName("conversation_id") val conversationId: String? = null,
    val kind: String,
    val title: String,
    val steps: List<StartProposalStep>,
    val state: String,
    val connect: Boolean = false,
    @SerialName("workflow_id") val workflowId: String? = null,
    @SerialName("created_at") val createdAt: String,
    @SerialName("decided_at") val decidedAt: String? = null,
) {
    val waiting: Boolean get() = state == "proposed"
}

@Serializable
data class StartProposalChoice(
    @SerialName("node_id") val nodeId: String? = null,
    val account: String? = null,
)

@Serializable
data class ApproveStartProposalRequest(val choices: Map<String, StartProposalChoice> = emptyMap())
