package mycelium.desktop

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import java.util.concurrent.CopyOnWriteArrayList
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.PrivacySettings
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertTrue

private const val ProvidedPrivacy = """{"voice_enabled":false,"speech_to_text_provider":{"provider_id":"p1","name":"Speech","model":"whisper-1"},"text_to_speech_provider":{"provider_id":"p1","name":"Speech","model":"tts-1"}}"""

private const val UnprovidedPrivacy = """{"voice_enabled":true,"speech_to_text_provider":null,"text_to_speech_provider":null}"""

class PrivacySettingsTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val sent = CopyOnWriteArrayList<Pair<String, String>>()

    private fun show(privacy: String) {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            sent += (request.method.value + " " + request.url.encodedPath) to String(request.body.toByteArray())

            respond(privacy, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        rule.setContent { MyceliumTheme(true) { PrivacySettings(store, QuietPlatform) } }
    }

    @Test
    fun usingVoiceSavesOnChange() {
        show(ProvidedPrivacy)
        rule.waitUntil(5_000) { sent.any { it.first == "GET /api/v1/privacy" } }

        rule.onNodeWithContentDescription("Use voice").performClick()
        rule.waitUntil(5_000) { sent.any { it.first == "PUT /api/v1/privacy" } }

        assertTrue(sent.first { it.first == "PUT /api/v1/privacy" }.second.contains("\"voice_enabled\":true"))
    }

    @Test
    fun theWakePhraseSaysItIsNotAvailableOnThisDevice() {
        show(ProvidedPrivacy)

        rule.onNodeWithText("The wake phrase is not available on this device.").assertExists()
    }

    @Test
    fun desktopSpeechToTextNamesYourProviderAndModel() {
        show(ProvidedPrivacy)
        rule.waitUntil(5_000) { sent.any { it.first == "GET /api/v1/privacy" } }
        rule.waitForIdle()

        rule.onNodeWithText("Speech, whisper-1").assertExists()
    }

    @Test
    fun withoutASpeechProviderTheErrorOffersProviders() {
        show(UnprovidedPrivacy)
        rule.waitUntil(5_000) { sent.any { it.first == "GET /api/v1/privacy" } }
        rule.waitForIdle()

        rule.onNodeWithText("Add a provider with a speech-to-text model in Providers to talk to Voice on this computer").assertExists()
        rule.onNodeWithText("Providers").assertExists()
    }
}
