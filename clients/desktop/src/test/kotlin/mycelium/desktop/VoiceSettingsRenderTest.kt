package mycelium.desktop

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.InternalComposeUiApi
import androidx.compose.ui.Modifier
import kotlinx.coroutines.runBlocking
import mycelium.AudioRecorder
import mycelium.Dictation
import mycelium.DictationProblem
import mycelium.Platform
import mycelium.WakeListener
import mycelium.api.Me
import mycelium.state.Store
import mycelium.ui.LocalPlatform
import mycelium.ui.MyTheme
import mycelium.ui.SettingsScreen
import mycelium.ui.SettingsSection
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertTrue

private const val OpenRouterSpeech = """{"provider_id":"p1","name":"OpenRouter","model":"mistralai/voxtral-mini-transcribe-3-0"}"""

private const val ExampleReading = """{"provider_id":"p2","name":"Example Speech","model":"voice-natural"}"""

private const val Providers = """[{"id":"p1","kind":"openai_compatible","name":"OpenRouter","base_url":"https://openrouter.ai/api/v1","models":["mistralai/voxtral-mini-transcribe-3-0"],"speech_to_text_models":["mistralai/voxtral-mini-transcribe-3-0"],"created_at":"2026-10-22T10:00:00Z"},{"id":"p2","kind":"openai_compatible","name":"Example Speech","base_url":"https://speech.example.com/v1","models":["voice-natural"],"text_to_speech_models":["voice-natural"],"created_at":"2026-10-22T10:00:00Z"}]"""

private const val VoiceAssistant = """[{"id":"a-voice","name":"Voice","preferences":{"models":[],"voice_name":"alloy","speech_speed":"normal"},"builtin_key":"voice","state":"idle","session_count":0,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z"}]"""

private fun privacy(speech: String, local: String, reading: String) = """{"voice_enabled":true,"speech_to_text_provider":$speech,"speech_to_text_local":$local,"text_to_speech_provider":$reading}"""

private fun local(state: String, downloaded: Long, total: Long, failure: String = "null") = """{"model_id":"example/speech-small","state":"$state","downloaded_bytes":$downloaded,"total_bytes":$total,"failure":$failure}"""

private object EveryVoiceFeature : Platform by QuietPlatform {
    override val hearsWakePhrase: Boolean get() = true

    override fun wakeListener(): WakeListener = object : WakeListener {
        override fun start(phrase: String, onHeard: () -> Unit) = true

        override fun stop() = Unit
    }

    override fun dictation(): Dictation = object : Dictation {
        override fun start(onHeard: (String) -> Unit, onStopped: (DictationProblem?) -> Unit) = Unit

        override fun stop() = Unit
    }

    override fun audioRecorder(): AudioRecorder = object : AudioRecorder {
        override fun start(onStopped: (DictationProblem?) -> Unit) = Unit

        override suspend fun stop(): ByteArray = ByteArray(0)
    }

    override fun loadPreference(key: String): String? = if (key == "voice-wake-phrase-enabled") "true" else null
}

@OptIn(InternalComposeUiApi::class)
class VoiceSettingsRenderTest {
    @get:Rule
    val scopes = TestScopes()

    private fun render(name: String, privacy: String, phone: Boolean = false, platform: Platform? = null) {
        val api = fakeApi(extra = mapOf("/experiments" to EnabledExperiments, "/privacy" to privacy, "/speech/voices" to """{"voices":["alloy","nova"]}""", "/model-providers" to Providers, "/assistants" to VoiceAssistant))
        val store = Store(api, scopes.create(), platform).also {
            runBlocking {
                it.refresh()
                it.voice.load()
                it.voice.loadVoices()
                it.loadAssistants()
            }
        }
        val rendered = renderAt(name, true, phone, height = if (phone) 1400 else 1240) {
            CompositionLocalProvider(LocalPlatform provides platform) {
                Box(Modifier.fillMaxSize().background(MyTheme.colors.bgContent)) {
                    SettingsScreen(store, Me("u", "sam@example.com", "Sam"), "https://mycelium.test", startOn = SettingsSection.Voice, onSignOut = {})
                }
            }
        }

        assertTrue(rendered.length() > 5_000)
    }

    @Test
    fun voiceSettingsRenderEveryStateOfTheLocalModel() {
        render("voice-provider", privacy(OpenRouterSpeech, "null", ExampleReading))
        render("voice-downloading", privacy("null", local("downloading", 46_000_000, 113_000_000), ExampleReading))
        render("voice-ready", privacy("null", local("ready", 113_000_000, 113_000_000), ExampleReading))
        render("voice-failed", privacy("null", local("failed", 0, 0, "\"Hugging Face has no model called example/speech-smal. Check the id and try again.\""), ExampleReading))
        render("voice-nothing", privacy("null", "null", "null"))
        render("voice-provider", privacy(OpenRouterSpeech, "null", ExampleReading), phone = true)
        render("voice-downloading", privacy("null", local("downloading", 46_000_000, 113_000_000), ExampleReading), phone = true)
        render("voice-every-feature", privacy(OpenRouterSpeech, "null", ExampleReading), platform = EveryVoiceFeature)
    }
}
