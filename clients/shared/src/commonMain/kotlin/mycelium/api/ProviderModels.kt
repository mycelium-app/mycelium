package mycelium.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
data class ModelProvider(
    val id: String,
    val kind: String,
    val name: String,
    @SerialName("base_url") val baseUrl: String? = null,
    @SerialName("key_hint") val keyHint: String? = null,
    val models: List<String> = emptyList(),
    @SerialName("default_model") val defaultModel: String? = null,
    @SerialName("created_at") val createdAt: String,
    val favourites: List<String> = emptyList(),
    @SerialName("model_class") val modelClass: String = "cloud",
    @SerialName("speech_model") val speechModel: String? = null,
)

@Serializable
data class Transcription(val text: String)

@Serializable
data class CreateModelProviderRequest(
    val kind: String,
    val name: String,
    @SerialName("base_url") val baseUrl: String? = null,
    @SerialName("api_key") val apiKey: String? = null,
)

@Serializable
data class UpdateModelProviderRequest(val name: String? = null, @SerialName("model_class") val modelClass: String? = null)

@Serializable
data class SetFavouriteModelRequest(val model: String, val favourite: Boolean)

@Serializable
data class CreateChatRequest(
    @SerialName("provider_id") val providerId: String,
    val model: String,
    val prompt: String? = null,
    @SerialName("parent_id") val parentId: String? = null,
)
