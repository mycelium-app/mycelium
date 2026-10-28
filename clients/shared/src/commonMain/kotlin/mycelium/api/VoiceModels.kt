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
data class PrivacySettings(
    @SerialName("voice_enabled") val voiceEnabled: Boolean = false,
    @SerialName("speech_to_text_provider") val speechToTextProvider: SpeechProvider? = null,
    @SerialName("text_to_speech_provider") val textToSpeechProvider: SpeechProvider? = null,
)

@Serializable
data class SavePrivacyRequest(@SerialName("voice_enabled") val voiceEnabled: Boolean)

@Serializable
data class SpeechRequest(val text: String, val voice: String? = null, val speed: String? = null)

@Serializable
data class SpeechVoices(val voices: List<String> = emptyList())

class SpokenAudio(val bytes: ByteArray, val mediaType: String)
