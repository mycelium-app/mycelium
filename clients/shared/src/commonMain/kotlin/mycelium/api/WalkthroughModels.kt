package mycelium.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

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
    val checks: List<WalkthroughCheck> = emptyList(),
)
