package mycelium.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
data class DecisionProvider(
    val id: String,
    val name: String,
    @SerialName("base_url") val baseUrl: String,
    @SerialName("key_hint") val keyHint: String? = null,
    val model: String,
    val uses: List<String> = emptyList(),
    @SerialName("model_provider_id") val modelProviderId: String? = null,
    @SerialName("created_at") val createdAt: String,
)

@Serializable
data class CreateDecisionProviderRequest(
    val name: String,
    @SerialName("base_url") val baseUrl: String = "",
    @SerialName("api_key") val apiKey: String? = null,
    val model: String,
    val uses: List<String>,
    @SerialName("model_provider_id") val modelProviderId: String? = null,
)

@Serializable
data class PatchDecisionProviderRequest(
    val name: String? = null,
    @SerialName("base_url") val baseUrl: String? = null,
    @SerialName("api_key") val apiKey: String? = null,
    val model: String? = null,
    val uses: List<String>? = null,
)

@Serializable
data class TestDecisionProviderRequest(
    @SerialName("base_url") val baseUrl: String,
    val key: String? = null,
)

@Serializable
data class DecisionProviderModels(val models: List<String> = emptyList())
