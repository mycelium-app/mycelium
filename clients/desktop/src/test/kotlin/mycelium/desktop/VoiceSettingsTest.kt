package mycelium.desktop

import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import java.util.concurrent.CopyOnWriteArrayList
import kotlinx.coroutines.runBlocking
import mycelium.AudioRecorder
import mycelium.DictationProblem
import mycelium.Platform
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.ExperimentsSettings
import mycelium.ui.ModelProvidersSettings
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Shell
import mycelium.ui.VoiceSettings
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private const val SpeechProviderJson = """{"id":"p1","kind":"openai_compatible","name":"Example Speech","base_url":"https://speech.example.com/v1","models":["chat-medium","house-dictation","transcribe-large","voice-natural"],"created_at":"2026-11-01T10:00:00Z","speech_to_text_models":["transcribe-large"],"text_to_speech_models":["voice-natural"]}"""

private const val OpenRouterJson = """{"id":"p3","kind":"openai_compatible","name":"OpenRouter","base_url":"https://openrouter.ai/api/v1","models":["anthropic/claude-sonnet-5.5","mistralai/voxtral-mini-transcribe-3-0"],"created_at":"2026-11-01T10:00:00Z","speech_to_text_models":["mistralai/voxtral-mini-transcribe-3-0"],"text_to_speech_models":[]}"""

private const val VoiceAssistantJson = """{"id":"a-voice","name":"Voice","preferences":{"models":[],"voice_name":"alloy","speech_speed":"normal"},"builtin_key":"voice","state":"idle","session_count":0,"instructions":"","created_at":"2026-11-01T10:00:00Z"}"""

private const val TranscribeRequest = "POST /api/v1/transcriptions"

private const val PutLocal = "PUT /api/v1/speech/local"

private fun privacyJson(speech: String = "null", local: String = "null", tts: String = "null") =
    """{"voice_enabled":true,"speech_to_text_provider":$speech,"speech_to_text_local":$local,"text_to_speech_provider":$tts}"""

private val ProvidedSpeech = """{"provider_id":"p1","name":"Example Speech","model":"transcribe-large"}"""

private val ProvidedReading = """{"provider_id":"p1","name":"Example Speech","model":"voice-natural"}"""

private fun localJson(state: String, downloaded: Long, total: Long?, failure: String? = null) =
    """{"model_id":"example/speech-small","state":"$state","downloaded_bytes":$downloaded,"total_bytes":${total ?: "null"},"failure":${failure?.let { "\"$it\"" } ?: "null"}}"""

private fun button(label: String) = hasContentDescription(label).and(SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.Button))

private class RecordingMicrophone : AudioRecorder {
    override fun start(onStopped: (DictationProblem?) -> Unit) = Unit

    override suspend fun stop(): ByteArray = byteArrayOf(1, 2, 3)
}

private object ServerPlatform : Platform by QuietPlatform {
    private val microphone = RecordingMicrophone()

    override fun audioRecorder(): AudioRecorder = microphone
}

class VoiceSettingsTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val sent = CopyOnWriteArrayList<Pair<String, String>>()
    private var privacy = privacyJson(ProvidedSpeech, tts = ProvidedReading)
    private var providers = "[$SpeechProviderJson]"
    private var experiments = EnabledExperiments.replace("\"voice\":true", "\"voice\":false")
    private var afterLocalChosen = privacyJson(local = localJson("downloading", 46_000_000, 113_000_000))

    private fun store(): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val body = String(request.body.toByteArray())

            sent += (request.method.value + " " + path) to body

            when {
                request.method == HttpMethod.Put && path.endsWith("/speech/local") -> {
                    privacy = afterLocalChosen
                    respond(privacy, HttpStatusCode.OK, json)
                }

                request.method == HttpMethod.Delete && path.endsWith("/speech/local") -> {
                    privacy = privacyJson()
                    respond(privacy, HttpStatusCode.OK, json)
                }

                request.method == HttpMethod.Put && path.contains("/speech/choices/") -> respond(privacy, HttpStatusCode.OK, json)
                request.method == HttpMethod.Put && path.endsWith("/experiments") -> respond(body, HttpStatusCode.OK, json)
                path.endsWith("/assistants/a-voice") -> respond(VoiceAssistantJson, HttpStatusCode.OK, json)
                request.method == HttpMethod.Post && path.endsWith("/transcriptions") -> respond("""{"text":"Remind me to review the pull request at four."}""", HttpStatusCode.OK, json)
                path.endsWith("/experiments") -> respond(experiments, HttpStatusCode.OK, json)
                path.endsWith("/privacy") -> respond(privacy, HttpStatusCode.OK, json)
                path.endsWith("/model-providers") -> respond(providers, HttpStatusCode.OK, json)
                path.endsWith("/speech/voices") -> respond("""{"voices":["alloy","verse"]}""", HttpStatusCode.OK, json)
                path.endsWith("/assistants") -> respond("[$VoiceAssistantJson]", HttpStatusCode.OK, json)
                path.endsWith("/nodes") || path.endsWith("/conversations") || path.endsWith("/decision-providers") -> respond("[]", HttpStatusCode.OK, json)
                else -> respond("{}", HttpStatusCode.OK, json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        runBlocking { store.refresh() }

        return store
    }

    private fun showVoice() {
        val store = store()

        rule.setContent { MyceliumTheme(true) { VoiceSettings(store, ServerPlatform) } }
        rule.waitUntil(5_000) { sent.any { it.first == "GET /api/v1/model-providers" } }
        rule.waitForIdle()
    }

    private fun present(text: String) = rule.onAllNodesWithText(text).fetchSemanticsNodes().isNotEmpty()

    @Test
    fun theVoicePageShowsAllFourStepsWithTheCurrentProviders() {
        showVoice()

        rule.onNodeWithText("How you talk").assertExists()
        rule.onNodeWithText("How speech becomes text").assertExists()
        rule.onNodeWithText("How replies are spoken").assertExists()
        rule.onNodeWithText("What leaves this machine").assertExists()
        rule.onNodeWithText("transcribe-large").assertExists()
        rule.onNodeWithText("voice-natural").assertExists()
        rule.onNodeWithContentDescription("Voice: alloy").assertExists()
        rule.onNodeWithText("Sent to Example Speech (speech.example.com) to turn into text.").assertExists()
        rule.onNodeWithText("Reply text is sent to Example Speech (speech.example.com).").assertExists()
        rule.onNodeWithText("No audio is stored.").assertExists()
    }

    @Test
    fun usingVoiceSavesOnChange() {
        showVoice()

        rule.onNodeWithContentDescription("Use voice").performClick()
        rule.waitUntil(5_000) { sent.any { it.first == "PUT /api/v1/privacy" } }

        assertTrue(sent.first { it.first == "PUT /api/v1/privacy" }.second.contains("\"voice_enabled\":false"))
    }

    @Test
    fun providersKeepsOnlyALinkToVoiceAndNoSpeechTable() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { ModelProvidersSettings(store) } }
        rule.waitUntil(5_000) { present("Example Speech") }

        rule.onNodeWithText("Speech to text and reading aloud are chosen in Voice.").assertExists()
        rule.onNode(button("Open Voice settings")).assertExists()
        rule.onNode(button("Change the provider for speech to text")).assertDoesNotExist()
        rule.onNodeWithText("transcribe-large").assertDoesNotExist()
    }

    @Test
    fun experimentsNoLongerListsVoiceInput() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { ExperimentsSettings(store) } }

        rule.onNodeWithContentDescription("History").assertExists()
        rule.onNodeWithContentDescription("Voice input").assertDoesNotExist()
    }

    @Test
    fun dictatingIntoTheComposerTogglesTheExperiment() {
        showVoice()

        rule.onNodeWithContentDescription("Dictate into the composer").performClick()
        rule.waitUntil(5_000) { sent.any { it.first == "PUT /api/v1/experiments" } }

        assertTrue(sent.first { it.first == "PUT /api/v1/experiments" }.second.contains("\"voice\":true"))
    }

    @Test
    fun theVoiceAssistantSettingsLinkToVoiceInsteadOfShowingVoiceAndSpeed() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { Shell(store, QuietPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.AssistantSettings("a-voice")) } }

        rule.waitUntil(5_000) { present("Voice settings") }
        rule.onNode(hasText("Voice settings").and(SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.Button))).assertExists()
        rule.onNodeWithText("Speaking voice").assertDoesNotExist()
        rule.onNodeWithText("Speaking speed").assertDoesNotExist()
        rule.onNodeWithText("Turn voice off").assertDoesNotExist()
    }

    @Test
    fun speakingSpeedSavesToTheVoiceAssistantAtOnce() {
        showVoice()

        rule.onNodeWithText("Faster").performClick()
        rule.waitUntil(5_000) { sent.any { it.first == "PATCH /api/v1/assistants/a-voice" } }

        assertTrue(sent.first { it.first == "PATCH /api/v1/assistants/a-voice" }.second.contains("\"speech_speed\":\"faster\""))
    }

    @Test
    fun choosingALocalModelStartsTheDownloadAndShowsItsProgress() {
        privacy = privacyJson()
        showVoice()

        rule.onNodeWithText("Choose speech to text").performClick()
        rule.onNodeWithText("A local model").performClick()
        rule.onNodeWithContentDescription("Hugging Face model").performTextInput("example/speech-small")
        rule.captureScreen("voice-dialog-local")
        rule.onNodeWithText("Download and use").performClick()
        rule.waitUntil(5_000) { sent.any { it.first == PutLocal } }

        assertEquals("""{"model_id":"example/speech-small"}""", sent.first { it.first == PutLocal }.second)
        rule.waitUntil(5_000) { present("Downloading, 46 MB of 113 MB") }
        rule.onNodeWithText("example/speech-small").assertExists()
        rule.onNodeWithText("Sent to your Mycelium server and turned into text there. It goes no further.").assertExists()
    }

    @Test
    fun aDownloadedLocalModelIsReadyOnTheServer() {
        privacy = privacyJson(local = localJson("ready", 113_000_000, 113_000_000))
        showVoice()

        rule.onNodeWithText("Ready, 113 MB on the server").assertExists()
        rule.onNode(button("Try speech to text")).assertExists()
    }

    @Test
    fun aFailedDownloadShowsTheFailureAndTryAgainAsksTheServerAgain() {
        privacy = privacyJson(local = localJson("failed", 0, null, "Hugging Face has no model with that id."))
        showVoice()

        rule.onNodeWithText("Could not download example/speech-small: Hugging Face has no model with that id.").assertExists()
        rule.onNode(button("Download example/speech-small again")).performClick()
        rule.waitUntil(5_000) { sent.any { it.first == PutLocal } }

        assertEquals("""{"model_id":"example/speech-small"}""", sent.first { it.first == PutLocal }.second)
    }

    @Test
    fun removingTheDownloadedModelAsksTheServerToDeleteIt() {
        privacy = privacyJson(local = localJson("ready", 113_000_000, 113_000_000))
        showVoice()

        rule.onNodeWithContentDescription("Change speech to text").performClick()
        rule.onNodeWithText("Remove the downloaded model").performClick()
        rule.waitUntil(5_000) { sent.any { it.first == "DELETE /api/v1/speech/local" } }
        rule.waitUntil(5_000) { present("Choose speech to text") }
    }

    @Test
    fun anEmptyModelIdAsksForOneWithoutContactingTheServer() {
        privacy = privacyJson()
        showVoice()

        rule.onNodeWithText("Choose speech to text").performClick()
        rule.onNodeWithText("A local model").performClick()
        rule.onNodeWithText("Download and use").performClick()

        rule.onNodeWithText("Enter a Hugging Face model id, such as owner/name.").assertExists()
        assertTrue(sent.none { it.first == PutLocal })
    }

    @Test
    fun theModelIdTheServerRefusesIsShownUnderTheField() {
        privacy = privacyJson()
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            when {
                request.method == HttpMethod.Put && path.endsWith("/speech/local") -> respond("""{"error":"local_model_invalid","message":"That is not a Hugging Face model id."}""", HttpStatusCode.BadRequest, json)
                path.endsWith("/privacy") -> respond(privacy, HttpStatusCode.OK, json)
                path.endsWith("/model-providers") -> respond("[]", HttpStatusCode.OK, json)
                else -> respond("{}", HttpStatusCode.OK, json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        rule.setContent { MyceliumTheme(true) { VoiceSettings(store, ServerPlatform) } }
        rule.waitUntil(5_000) { present("Choose speech to text") }
        rule.onNodeWithText("Choose speech to text").performClick()
        rule.onNodeWithText("A local model").performClick()
        rule.onNodeWithContentDescription("Hugging Face model").performTextInput("nonsense")
        rule.onNodeWithText("Download and use").performClick()

        rule.waitUntil(5_000) { present("That is not a Hugging Face model id.") }
    }

    @Test
    fun withNothingSetUpTheStepSaysWhatToChooseAndTheServerLineSaysNothingIsSent() {
        privacy = privacyJson()
        showVoice()

        rule.onNodeWithText("Nothing turns speech into text yet. Choose a provider with a speech-to-text model, such as Voxtral on OpenRouter, or a local model on your server.").assertExists()
        rule.onNodeWithText("Nothing is sent until speech to text is chosen.").assertExists()
    }

    @Test
    fun anOpenRouterTranscriptionModelIsListedAmongTheSpeechToTextModels() {
        providers = "[$OpenRouterJson]"
        privacy = privacyJson("""{"provider_id":"p3","name":"OpenRouter","model":"mistralai/voxtral-mini-transcribe-3-0"}""")
        showVoice()

        rule.onNodeWithContentDescription("Change speech to text").performClick()
        rule.onNodeWithContentDescription("Model: mistralai/voxtral-mini-transcribe-3-0").performClick()

        rule.onNodeWithText("Speech-to-text models").assertExists()
        rule.onNodeWithText("Other models").assertExists()
        assertTrue(rule.onAllNodesWithText("mistralai/voxtral-mini-transcribe-3-0").fetchSemanticsNodes().size >= 3)
        rule.captureScreen("voice-dialog-openrouter")
    }

    @Test
    fun tryingSpeechToTextShowsWhatWasHeard() {
        showVoice()

        rule.onNode(button("Try speech to text")).performClick()
        rule.onNode(button("Stop speech to text")).performClick()
        rule.waitUntil(5_000) { present("Remind me to review the pull request at four.") }

        assertTrue(sent.any { it.first == TranscribeRequest })
    }

    @Test
    fun theOldPrivacyAddressOpensVoice() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { Shell(store, QuietPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Settings) } }
        rule.waitForIdle()

        rule.runOnIdle { store.openScreen("settings/privacy") }

        rule.waitUntil(5_000) { present("How you talk") }
        rule.onNodeWithText("Use voice").assertExists()
    }
}
