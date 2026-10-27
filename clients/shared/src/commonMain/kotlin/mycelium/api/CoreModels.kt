package mycelium.api

import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json

val MyceliumJson = Json {
    ignoreUnknownKeys = true
    explicitNulls = false
    encodeDefaults = false
}

@Serializable
data class ErrorBody(val error: String, val message: String)

@Serializable
data class ServerVersion(val version: String)
