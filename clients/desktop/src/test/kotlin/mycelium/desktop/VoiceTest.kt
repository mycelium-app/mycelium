package mycelium.desktop

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onLast
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import java.util.concurrent.CopyOnWriteArrayList
import kotlin.test.Test
import kotlin.test.assertTrue
import kotlinx.coroutines.runBlocking
import mycelium.Dictation
import mycelium.DictationProblem
import mycelium.Platform
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.state.VoicePhase
import mycelium.state.VoiceProblem
import mycelium.state.VoiceState
import mycelium.state.VoiceTurn
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Shell
import mycelium.ui.VoiceCard
import mycelium.ui.VoiceCardActions
import org.junit.Rule

private const val VoiceOn = """{"voice_enabled":true,"speech_to_text_provider":null,"text_to_speech_provider":null}"""

private const val VoiceOff = """{"voice_enabled":false}"""

private const val VoiceAssistantJson = """{"id":"a-voice","name":"Voice","builtin_key":"voice","created_at":"2026-10-22T10:00:00Z"}"""

private const val VoiceThreadJson = """{"id":"c-voice","owner":"u","node_id":null,"agent":"chat","cwd":"","title":"Voice","status":"working",
"acp_session_id":null,"mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null}"""

private class HeardDictation : Dictation {
    var onHeard: (String) -> Unit = {}
    var onStopped: (DictationProblem?) -> Unit = {}

    override fun start(onHeard: (String) -> Unit, onStopped: (DictationProblem?) -> Unit) {
        this.onHeard = onHeard
        this.onStopped = onStopped
    }

    override fun stop() = onStopped(null)
}

private class MicrophonePlatform(private val dictation: Dictation) : Platform by QuietPlatform {
    override fun dictation(): Dictation = dictation
}

private val NoActions = VoiceCardActions({}, {}, {}, {}, {}, {}, {}, {}, {}, null, {})

class VoiceTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val requests = CopyOnWriteArrayList<Pair<String, String>>()

    private fun open(privacy: String, dictation: Dictation = HeardDictation()) {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            requests += (request.method.value + " " + path) to String(request.body.toByteArray())

            when {
                path.endsWith("/privacy") -> respond(privacy, HttpStatusCode.OK, json)
                path.endsWith("/assistants/builtin/voice") -> respond(VoiceAssistantJson, HttpStatusCode.OK, json)
                path.endsWith("/prompt") && request.method == HttpMethod.Post -> respond(VoiceThreadJson, HttpStatusCode.OK, json)
                path.endsWith("/conversations") || path.endsWith("/assistants") || path.endsWith("/nodes") || path.endsWith("/events") -> respond("[]", HttpStatusCode.OK, json)
                else -> respond("{}", HttpStatusCode.OK, json)
            }
        }
        val platform = MicrophonePlatform(dictation)
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create(), platform)

        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { Shell(store, platform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Empty) } }
    }

    private fun card(state: VoiceState) {
        rule.setContent { MyceliumTheme(true) { VoiceCard(state, NoActions) } }
    }

    private fun microphones() = rule.onAllNodesWithContentDescription("Talk to Voice").fetchSemanticsNodes().size

    @Test
    fun theMicrophoneIsHiddenWhileVoiceIsOff() {
        open(VoiceOff)
        rule.waitForIdle()

        assertTrue(microphones() == 0)
    }

    @Test
    fun theMicrophoneShowsWhileVoiceIsOn() {
        open(VoiceOn)
        rule.waitForIdle()

        rule.onNodeWithContentDescription("Talk to Voice").assertExists()
    }

    @Test
    fun pressingTheMicrophoneOpensTheCardListening() {
        open(VoiceOn)

        rule.onNodeWithContentDescription("Talk to Voice").performClick()

        rule.onAllNodesWithText("Stop listening").onLast().assertExists()
        rule.onNodeWithText("Cancel").assertExists()
        rule.onNodeWithContentDescription("Stop listening").assertExists()
    }

    @Test
    fun stoppingListeningSendsWhatWasHeardAsASpokenMessage() {
        val dictation = HeardDictation()
        open(VoiceOn, dictation)

        rule.onNodeWithContentDescription("Talk to Voice").performClick()
        rule.runOnIdle { dictation.onHeard("open the usage history") }
        rule.onNodeWithText("open the usage history").assertExists()
        rule.onAllNodesWithText("Stop listening").onFirst().performClick()
        rule.waitUntil(5_000) { requests.any { it.first.endsWith("/assistants/a-voice/prompt") } }

        val sent = requests.first { it.first.endsWith("/prompt") }.second

        assertTrue(sent.contains("\"text\":\"open the usage history\"") && sent.contains("\"spoken\":true"))
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Thinking").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun cancellingDiscardsWhatWasHeard() {
        val dictation = HeardDictation()
        open(VoiceOn, dictation)

        rule.onNodeWithContentDescription("Talk to Voice").performClick()
        rule.runOnIdle { dictation.onHeard("send the money") }
        rule.onNodeWithText("Cancel").performClick()
        rule.waitForIdle()

        assertTrue(requests.none { it.first.endsWith("/prompt") })
    }

    @Test
    fun aBlockedMicrophoneSaysHowToAllowItOnDesktop() {
        card(VoiceState(open = true, phase = VoicePhase.Problem(VoiceProblem.MicrophoneDenied)))

        rule.onNodeWithText("Mycelium cannot use the microphone. Allow microphone access for Mycelium in your system settings, then select Try again.").assertExists()
        rule.onNodeWithText("Try again").assertExists()
        rule.onNodeWithText("Type instead").assertExists()
    }

    @Test
    fun aMissingMicrophoneSaysToConnectOne() {
        card(VoiceState(open = true, phase = VoicePhase.Problem(VoiceProblem.NoMicrophone)))

        rule.onNodeWithText("No microphone found. Connect one, then select Try again.").assertExists()
    }

    @Test
    fun withoutASpeechProviderTheCardOffersProvidersInsteadOfTryAgain() {
        card(VoiceState(open = true, phase = VoicePhase.Problem(VoiceProblem.NoSpeechProvider)))

        rule.onNodeWithText("Providers").assertExists()
        assertTrue(rule.onAllNodesWithContentDescription("Try again").fetchSemanticsNodes().isEmpty())
    }

    @Test
    fun theSpeakingCardOffersToStopAndOpenTheConversation() {
        card(VoiceState(open = true, phase = VoicePhase.Speaking, turn = VoiceTurn(said = "what is next", reply = "Review the draft.", conversationId = "c-voice")))

        rule.onNodeWithText("what is next").assertExists()
        rule.onNodeWithText("Review the draft.").assertExists()
        rule.onNodeWithText("Speaking").assertExists()
        rule.onNodeWithText("Stop speaking").assertExists()
        rule.onNodeWithText("Open conversation").assertExists()
    }

    @Test
    fun afterSpeechEndsTheCardKeepsTalkAgain() {
        card(VoiceState(open = true, phase = VoicePhase.Ended, turn = VoiceTurn(said = "what is next", reply = "Review the draft.", conversationId = "c-voice")))

        rule.onNodeWithText("Talk again").assertExists()
    }
}
