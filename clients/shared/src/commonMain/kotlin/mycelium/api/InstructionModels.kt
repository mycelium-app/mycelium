package mycelium.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

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
data class DeclineInstructionProposalRequest(val note: String? = null, val redo: Boolean = false, val notify: Boolean = true)

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
