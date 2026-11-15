package mycelium.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonObject

@Serializable
data class ClientEvent(
    val at: String,
    val kind: String,
    val message: String = "",
    val detail: JsonObject = JsonObject(emptyMap()),
)

@Serializable
data class ClientEventReport(
    @SerialName("session_id") val sessionId: String,
    val client: String,
    val version: String,
    val events: List<ClientEvent>,
)
