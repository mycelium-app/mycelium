package mycelium.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

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
