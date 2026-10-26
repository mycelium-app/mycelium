package mycelium.desktop

import androidx.compose.ui.test.assertTextEquals
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.client.request.HttpRequestData
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import java.util.concurrent.CopyOnWriteArrayList
import kotlinx.coroutines.runBlocking
import mycelium.AudioRecorder
import mycelium.Dictation
import mycelium.DictationProblem
import mycelium.Platform
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Shell
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertContentEquals
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private const val IdleJson = """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/app","title":"Fix the build","status":"done",
"acp_session_id":"s","mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null}"""

private const val SpeechProvider = """[{"id":"p1","kind":"openai_compatible","name":"Speech","models":["whisper-1"],"created_at":"2026-10-22T10:00:00Z","speech_model":"whisper-1"}]"""

private const val ChatOnlyProvider = """[{"id":"p1","kind":"openai_compatible","name":"Chat","models":["gpt-4o-mini"],"created_at":"2026-10-22T10:00:00Z"}]"""

private class FakeDictation : Dictation {
    var onHeard: (String) -> Unit = {}
    var onStopped: (DictationProblem?) -> Unit = {}

    override fun start(onHeard: (String) -> Unit, onStopped: (DictationProblem?) -> Unit) {
        this.onHeard = onHeard
        this.onStopped = onStopped
    }

    override fun stop() = onStopped(null)
}

private class FakeRecorder(private val recording: ByteArray) : AudioRecorder {
    override fun start(onStopped: (DictationProblem?) -> Unit) = Unit

    override suspend fun stop(): ByteArray = recording
}

private class SpeakingPlatform(private val dictation: Dictation? = null, private val recorder: AudioRecorder? = null) : Platform by QuietPlatform {
    override fun dictation(): Dictation? = dictation

    override fun audioRecorder(): AudioRecorder? = recorder
}

class DictationTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val requests = CopyOnWriteArrayList<Pair<HttpRequestData, ByteArray>>()

    private fun open(platform: Platform, voice: Boolean = true, providers: String = "[]") {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val experiments = if (voice) EnabledExperiments else EnabledExperiments.replace("\"voice\":true", "\"voice\":false")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            requests += request to request.body.toByteArray()

            when {
                path.endsWith("/experiments") -> respond(experiments, HttpStatusCode.OK, json)
                path.endsWith("/transcriptions") && request.method == HttpMethod.Post -> respond("""{"text":"fix the build"}""", HttpStatusCode.OK, json)
                path.endsWith("/model-providers") -> respond(providers, HttpStatusCode.OK, json)
                path.endsWith("/conversations") -> respond("[$IdleJson]", HttpStatusCode.OK, json)
                path.endsWith("/conversations/c1") -> respond(IdleJson, HttpStatusCode.OK, json)
                path.endsWith("/events") || path.endsWith("/queue") || path.endsWith("/nodes") -> respond("[]", HttpStatusCode.OK, json)
                else -> respond("{}", HttpStatusCode.OK, json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { Shell(store, platform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Chat("c1")) } }
    }

    private fun dictateButtons() = rule.onAllNodesWithContentDescription("Dictate").fetchSemanticsNodes().size

    @Test
    fun dictated_words_land_in_the_draft_at_the_cursor() {
        val dictation = FakeDictation()
        open(SpeakingPlatform(dictation = dictation))

        rule.onNode(hasSetTextAction()).performTextInput("Please")
        rule.onNodeWithContentDescription("Dictate").performClick()
        rule.runOnIdle { dictation.onHeard("fix the build") }

        rule.onNode(hasSetTextAction()).assertTextEquals("Please fix the build")
    }

    @Test
    fun the_button_reads_stop_dictating_while_listening() {
        val dictation = FakeDictation()
        open(SpeakingPlatform(dictation = dictation))

        rule.onNodeWithContentDescription("Dictate").performClick()
        rule.onNodeWithContentDescription("Stop dictating").performClick()

        rule.onNodeWithContentDescription("Dictate").assertExists()
    }

    @Test
    fun denied_microphone_access_says_how_to_allow_it() {
        val dictation = FakeDictation()
        open(SpeakingPlatform(dictation = dictation))

        rule.onNodeWithContentDescription("Dictate").performClick()
        rule.runOnIdle { dictation.onStopped(DictationProblem.Denied) }

        rule.onNodeWithText("Allow microphone access for Mycelium to dictate.").assertExists()
    }

    @Test
    fun desktop_records_and_sends_the_audio_to_the_speech_provider() {
        val recording = byteArrayOf(1, 2, 3, 4)
        open(SpeakingPlatform(recorder = FakeRecorder(recording)), providers = SpeechProvider)

        rule.waitUntil(5_000) { dictateButtons() == 1 }
        rule.onNodeWithContentDescription("Dictate").performClick()
        rule.onNodeWithContentDescription("Stop dictating").performClick()
        rule.waitUntil(5_000) { requests.any { it.first.method == HttpMethod.Post && it.first.url.encodedPath.endsWith("/transcriptions") } }

        val sent = requests.first { it.first.url.encodedPath.endsWith("/transcriptions") }
        assertEquals("audio/wav", sent.first.body.contentType?.toString())
        assertContentEquals(recording, sent.second)
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Dictate").fetchSemanticsNodes().isNotEmpty() }
        rule.onNode(hasSetTextAction()).assertTextEquals("fix the build")
    }

    @Test
    fun no_dictate_button_without_a_speech_provider() {
        open(SpeakingPlatform(recorder = FakeRecorder(byteArrayOf(1))), providers = ChatOnlyProvider)

        rule.waitUntil(5_000) { requests.any { it.first.url.encodedPath.endsWith("/model-providers") } }
        rule.waitForIdle()

        assertEquals(0, dictateButtons())
    }

    @Test
    fun no_dictate_button_with_the_experiment_off() {
        open(SpeakingPlatform(dictation = FakeDictation(), recorder = FakeRecorder(byteArrayOf(1))), voice = false, providers = SpeechProvider)

        rule.waitForIdle()

        assertTrue(dictateButtons() == 0)
    }
}
