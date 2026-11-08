package mycelium.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
data class InboxPost(
    val id: String,
    val author: String,
    val kind: String,
    val message: String,
    val project: String? = null,
    val url: String? = null,
    @SerialName("created_at") val createdAt: String,
)

@Serializable
data class InboxFeed(
    val configured: Boolean,
    val posts: List<InboxPost> = emptyList(),
    val error: String? = null,
    @SerialName("stale_since") val staleSince: String? = null,
)
