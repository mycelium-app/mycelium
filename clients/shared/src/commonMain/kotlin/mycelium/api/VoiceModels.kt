package mycelium.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
data class SpeechProvider(
    @SerialName("provider_id") val providerId: String,
    val name: String,
    val model: String,
)

@Serializable
data class LocalSpeechModel(
    @SerialName("model_id") val modelId: String,
    val state: String,
    @SerialName("downloaded_bytes") val downloadedBytes: Long = 0,
    @SerialName("total_bytes") val totalBytes: Long? = null,
    val failure: String? = null,
)

@Serializable
data class ChooseLocalSpeechModelRequest(@SerialName("model_id") val modelId: String)

@Serializable
data class PrivacySettings(
    @SerialName("voice_enabled") val voiceEnabled: Boolean = false,
    @SerialName("speech_to_text_provider") val speechToTextProvider: SpeechProvider? = null,
    @SerialName("speech_to_text_local") val speechToTextLocal: LocalSpeechModel? = null,
    @SerialName("text_to_speech_provider") val textToSpeechProvider: SpeechProvider? = null,
)

@Serializable
data class ChooseSpeechModelRequest(@SerialName("provider_id") val providerId: String, val model: String)

@Serializable
data class SavePrivacyRequest(@SerialName("voice_enabled") val voiceEnabled: Boolean)

@Serializable
data class SpeechRequest(val text: String, val voice: String? = null, val speed: String? = null)

@Serializable
data class SpeechVoices(val voices: List<String> = emptyList())

class SpokenAudio(val bytes: ByteArray, val mediaType: String)
