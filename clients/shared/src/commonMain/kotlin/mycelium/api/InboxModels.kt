package mycelium.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
data class InboxPost(
    val id: String,
    val author: String,
    val message: String,
    val project: String? = null,
    @SerialName("conversation_id") val conversationId: String,
    @SerialName("conversation_title") val conversationTitle: String,
    @SerialName("created_at") val createdAt: String,
)

@Serializable
data class InboxFeed(
    val posts: List<InboxPost> = emptyList(),
)
