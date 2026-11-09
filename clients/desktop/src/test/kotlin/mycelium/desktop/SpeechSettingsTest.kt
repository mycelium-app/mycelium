package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.getUnclippedBoundsInRoot
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.unit.dp
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import java.util.concurrent.CopyOnWriteArrayList
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.ModelProvidersSettings
import mycelium.ui.MyceliumTheme
import mycelium.ui.NoSpeechToTextStatement
import mycelium.ui.Route
import mycelium.ui.Shell
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private const val SpeechProviderJson = """{"id":"p1","kind":"openai_compatible","name":"Example Speech","base_url":"https://speech.example.com/v1","models":["chat-medium","house-dictation","transcribe-large","voice-natural"],"created_at":"2026-11-01T10:00:00Z","speech_to_text_models":["transcribe-large"],"text_to_speech_models":[]}"""
private const val AnthropicJson = """{"id":"p2","kind":"anthropic","name":"Anthropic","models":["claude-test"],"created_at":"2026-11-01T10:00:00Z"}"""

private fun privacyJson(speechModel: String?, ttsModel: String?): String {
    val speech = speechModel?.let { """{"provider_id":"p1","name":"Example Speech","model":"$it"}""" } ?: "null"
    val tts = ttsModel?.let { """{"provider_id":"p1","name":"Example Speech","model":"$it"}""" } ?: "null"

    return """{"voice_enabled":true,"speech_to_text_provider":$speech,"text_to_speech_provider":$tts}"""
}

private fun button(label: String) = hasContentDescription(label).and(SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.Button))

class SpeechSettingsTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val chosen = CopyOnWriteArrayList<Pair<String, String>>()
    private var providers = "[$SpeechProviderJson]"
    private var privacy = privacyJson("transcribe-large", "voice-natural")

    private fun store(): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            when {
                request.method == HttpMethod.Put && path.contains("/speech/choices/") -> {
                    chosen += path.substringAfterLast('/') to String(request.body.toByteArray())
                    privacy = if (path.endsWith("/text_to_speech")) privacyJson("transcribe-large", "voice-natural") else privacyJson("house-dictation", "voice-natural")
                    respond(privacy, HttpStatusCode.OK, json)
                }

                path.endsWith("/privacy") -> respond(privacy, HttpStatusCode.OK, json)
                path.endsWith("/model-providers") -> respond(providers, HttpStatusCode.OK, json)
                path.endsWith("/nodes") || path.endsWith("/conversations") || path.endsWith("/decision-providers") -> respond("[]", HttpStatusCode.OK, json)
                else -> respond("{}", HttpStatusCode.OK, json)
            }
        }

        return Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
    }

    @Test
    fun eachSpeechUseShowsItsProviderAndModelWithChangeOnTheRow() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { ModelProvidersSettings(store) } }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("transcribe-large").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Speech").assertExists()
        rule.onNodeWithText("voice-natural").assertExists()
        rule.onNode(button("Change the provider for speech to text")).assertExists()
        rule.onNode(button("Change the provider for reading replies aloud")).assertExists()
    }

    @Test
    fun changingSpeechToTextListsRecognisedModelsFirstAndSavesTheChoice() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { ModelProvidersSettings(store) } }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("transcribe-large").fetchSemanticsNodes().isNotEmpty() }
        rule.onNode(button("Change the provider for speech to text")).performClick()
        rule.onNodeWithContentDescription("Model: transcribe-large").performClick()
        rule.onNodeWithText("Speech-to-text models").assertExists()
        rule.onNodeWithText("Other models").assertExists()
        rule.onNode(hasText("house-dictation")).performClick()
        rule.onNodeWithText("Save").performClick()

        rule.waitUntil(5_000) { chosen.isNotEmpty() }
        assertEquals("speech_to_text" to """{"provider_id":"p1","model":"house-dictation"}""", chosen.single())
        rule.waitUntil(5_000) { rule.onAllNodesWithText("house-dictation").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun withNoSpeechToTextProviderTheRowSaysWhatToAddAndOpensTheAddForm() {
        providers = "[$AnthropicJson]"
        privacy = privacyJson(null, null)
        val store = store()
        rule.setContent { MyceliumTheme(true) { ModelProvidersSettings(store) } }

        rule.waitUntil(5_000) { rule.onAllNodesWithText(NoSpeechToTextStatement).fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Add a provider with a text-to-speech model to hear replies.").assertExists()
        rule.onNode(button("Choose a speech-to-text model from your providers")).assertDoesNotExist()
        rule.onNode(button("Choose a text-to-speech model from your providers")).assertDoesNotExist()
        rule.onNode(button("Add a provider for speech to text")).performClick()
        rule.onNodeWithText("Needs an address that offers a speech-to-text model, such as whisper-1.").assertExists()
    }

    @Test
    fun aProviderWithNoRecognisedTextToSpeechModelCanStillHaveAModelChosenForReadingAloud() {
        privacy = privacyJson("transcribe-large", null)
        val store = store()
        rule.setContent { MyceliumTheme(true) { ModelProvidersSettings(store) } }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Add a provider with a text-to-speech model to hear replies.").fetchSemanticsNodes().isNotEmpty() }
        rule.onNode(button("Choose a text-to-speech model from your providers")).performClick()
        rule.onNodeWithContentDescription("Model: Model").performClick()
        rule.onNodeWithText("Other models").assertExists()
        rule.onNode(hasText("voice-natural")).performClick()
        rule.onNodeWithText("Save").performClick()

        rule.waitUntil(5_000) { chosen.isNotEmpty() }
        assertEquals("text_to_speech" to """{"provider_id":"p1","model":"voice-natural"}""", chosen.single())
        rule.waitUntil(5_000) { rule.onAllNodesWithText("voice-natural").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun onAPhoneWidthTheProviderAndModelStackUnderTheNameAndChangeStaysOnTheRow() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { Box(Modifier.width(390.dp)) { ModelProvidersSettings(store) } } }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("transcribe-large").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("transcribe-large").assertIsDisplayed()
        rule.onNodeWithText("voice-natural").assertIsDisplayed()
        rule.onNode(button("Change the provider for speech to text")).assertIsDisplayed()
    }

    @Test
    fun justAboveThePhoneWidthChangeStillFitsOnTheRow() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { Box(Modifier.width(484.dp)) { ModelProvidersSettings(store) } } }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("transcribe-large").fetchSemanticsNodes().isNotEmpty() }
        rule.waitForIdle()
        val change = rule.onNode(button("Change the provider for speech to text")).getUnclippedBoundsInRoot()
        assertTrue(change.right <= 484.dp, "Change ends at ${change.right}")
    }

    @Test
    fun openingSpeechSetupScrollsTheSectionIntoView() {
        val store = store()
        rule.setContent {
            MyceliumTheme(true) {
                Column(Modifier.height(240.dp).verticalScroll(rememberScrollState())) {
                    Spacer(Modifier.height(2_000.dp))
                    ModelProvidersSettings(store, revealSpeech = true)
                }
            }
        }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("transcribe-large").fetchSemanticsNodes().isNotEmpty() }
        rule.waitForIdle()
        rule.onNodeWithText("Speech").assertIsDisplayed()
    }

    @Test
    fun setUpSpeechToTextOpensProvidersEvenFromAnotherSettingsSection() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { Shell(store, PlatformStub, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Settings) } }
        rule.waitForIdle()

        rule.runOnIdle { store.openScreen("settings/privacy") }
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Use voice").fetchSemanticsNodes().isNotEmpty() }
        rule.runOnIdle { store.openScreen("settings/providers/speech") }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("transcribe-large").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Speech").assertExists()
    }
}
