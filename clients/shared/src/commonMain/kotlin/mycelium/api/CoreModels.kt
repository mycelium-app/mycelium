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
data class RemoteImageError(val error: String, val message: String, val host: String? = null)

@Serializable
data class RemoteImageRequest(val url: String)

sealed interface RemoteImageResult {
    class Bytes(val bytes: ByteArray) : RemoteImageResult

    data class NeedsClick(val host: String) : RemoteImageResult

    data class Refused(val message: String, val privateAddress: Boolean = false) : RemoteImageResult
}

@Serializable
data class ServerVersion(val version: String)
