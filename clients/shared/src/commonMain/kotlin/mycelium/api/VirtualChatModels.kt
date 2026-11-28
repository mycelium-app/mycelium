package mycelium.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull

@Serializable
data class VirtualChat(
    val id: String,
    @SerialName("assistant_id") val assistantId: String,
    val title: String,
    val description: String? = null,
    val closed: Boolean = false,
    val unread: Boolean = false,
    val todos: JsonElement = JsonNull,
    @SerialName("created_at") val createdAt: String,
    @SerialName("updated_at") val updatedAt: String,
)

@Serializable
data class VirtualMessage(
    val id: String,
    @SerialName("chat_id") val chatId: String,
    @SerialName("from_person") val fromPerson: Boolean,
    val text: String,
    val attachments: List<Attachment> = emptyList(),
    val sending: Boolean = false,
    @SerialName("created_at") val createdAt: String,
)

@Serializable
data class CreateVirtualChatRequest(val title: String)

@Serializable
data class PatchVirtualChatRequest(val closed: Boolean? = null, val read: Boolean = false)

@Serializable
data class SendVirtualMessageRequest(val text: String, val attachments: List<Attachment> = emptyList())
