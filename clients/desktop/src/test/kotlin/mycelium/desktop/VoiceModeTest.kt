package mycelium.desktop

import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
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
import kotlin.test.assertEquals
import kotlin.test.assertTrue
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.builtins.serializer
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import mycelium.AudioPlayer
import mycelium.Dictation
import mycelium.DictationProblem
import mycelium.Platform
import mycelium.api.Event
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.AppState
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Shell
import org.junit.Rule

private const val VoiceWithReading = """{"voice_enabled":true,"speech_to_text_provider":null,"text_to_speech_provider":{"provider_id":"p1","name":"Speech","model":"tts-1"}}"""

private const val Conversation = """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/app","title":"Restore flow copy","status":"done","acp_session_id":"s1","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z"}"""

private const val SpokenReply = "Two tests still fail on `restore-flow`:\n\n- picker keeps the selection\n- restore button wording\n\n```mycelium-spoken\nTwo restore tests still fail, both in the snapshot picker.\n```"

private fun chunk(seq: Long, text: String) = Event("c1", seq, "acp_update", Json.parseToJsonElement("""{"sessionId":"s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":${Json.encodeToString(String.serializer(), text)}}}}"""))

private fun replied(said: String, reply: String) = listOf(
    Event("c1", 1, "user_message", Json.parseToJsonElement("""{"text":"$said","spoken":true}""")),
    chunk(2, reply),
    Event("c1", 3, "turn_end", Json.parseToJsonElement("""{}""")),
)

private class ListeningDictation : Dictation {
    var onHeard: (String) -> Unit = {}
    var onStopped: (DictationProblem?) -> Unit = {}
    var starts = 0

    override fun start(onHeard: (String) -> Unit, onStopped: (DictationProblem?) -> Unit) {
        starts += 1
        this.onHeard = onHeard
        this.onStopped = onStopped
    }

    override fun stop() = onStopped(null)
}

private class RecordingPlayer : AudioPlayer {
    val played = CopyOnWriteArrayList<ByteArray>()

    override fun play(audio: ByteArray, mediaType: String, onDone: (Boolean) -> Unit) {
        played += audio
        onDone(true)
    }

    override fun stop() = Unit
}

private class VoiceModePlatform(private val dictation: Dictation, private val player: AudioPlayer) : Platform by QuietPlatform {
    override fun dictation(): Dictation = dictation

    override fun audioPlayer(): AudioPlayer = player
}

class VoiceModeTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val requests = CopyOnWriteArrayList<Pair<String, String>>()
    private val dictation = ListeningDictation()
    private val player = RecordingPlayer()

    private fun open(events: String = "[]"): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            requests += (request.method.value + " " + path) to String(request.body.toByteArray())

            when {
                path.endsWith("/privacy") -> respond(VoiceWithReading, HttpStatusCode.OK, json)
                path.endsWith("/speech") -> respond(byteArrayOf(1, 2, 3), HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "audio/wav"))
                path.endsWith("/conversations") -> respond("[$Conversation]", HttpStatusCode.OK, json)
                path.endsWith("/conversations/c1") -> respond(Conversation, HttpStatusCode.OK, json)
                path.endsWith("/prompt") && request.method == HttpMethod.Post -> respond("", HttpStatusCode.NoContent)
                path.endsWith("/events") -> respond(events, HttpStatusCode.OK, json)
                path.endsWith("/queue") || path.endsWith("/nodes") || path.endsWith("/assistants") -> respond("[]", HttpStatusCode.OK, json)
                else -> respond("{}", HttpStatusCode.OK, json)
            }
        }
        val platform = VoiceModePlatform(dictation, player)
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create(), platform)

        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { Shell(store, platform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Chat("c1")) } }
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Voice mode").fetchSemanticsNodes().isNotEmpty() }

        return store
    }

    private fun patches() = requests.filter { it.first == "PATCH /api/v1/conversations/c1" }.map { it.second }

    private fun spokenTexts() = requests.filter { it.first.endsWith("/speech") }.map { Json.parseToJsonElement(it.second).jsonObject["text"]!!.jsonPrimitive.content }

    private fun voiceMode(state: String) = rule.onNodeWithContentDescription("Voice mode").assert(SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, state))

    private fun enterAndAsk(store: Store, said: String) {
        rule.onNodeWithContentDescription("Voice mode").performClick()
        rule.waitUntil(5_000) { dictation.starts == 1 }
        rule.runOnIdle { dictation.onHeard(said) }
        rule.onNodeWithContentDescription("Send").performClick()
        rule.waitUntil(5_000) { requests.any { it.first.endsWith("/conversations/c1/prompt") } }
        rule.waitUntil(5_000) { store.voice.state.value.turn.conversationId == "c1" }
    }

    @Test
    fun enteringVoiceModeTellsTheServerListensAndReplacesDictate() {
        open()
        voiceMode("Off")

        rule.onNodeWithContentDescription("Voice mode").performClick()

        rule.waitUntil(5_000) { patches().isNotEmpty() }
        assertEquals("""{"voice_mode":true}""", patches().single())
        voiceMode("On, Listening")
        assertEquals(1, dictation.starts)
        assertTrue(rule.onAllNodesWithContentDescription("Dictate").fetchSemanticsNodes().isEmpty())
    }

    @Test
    fun leavingVoiceModeTellsTheServer() {
        open()
        rule.onNodeWithContentDescription("Voice mode").performClick()
        rule.waitUntil(5_000) { patches().size == 1 }

        rule.onNodeWithContentDescription("Voice mode").performClick()

        rule.waitUntil(5_000) { patches().size == 2 }
        assertEquals("""{"voice_mode":false}""", patches().last())
        voiceMode("Off")
    }

    @Test
    fun sendWithAnEmptyFieldSendsWhatWasHeardToThisConversationAsSpoken() {
        val store = open()

        enterAndAsk(store, "Which tests are failing")

        val sent = requests.first { it.first.endsWith("/conversations/c1/prompt") }.second
        assertTrue(sent.contains("\"text\":\"Which tests are failing\"") && sent.contains("\"spoken\":true"), sent)
        assertTrue(requests.none { it.first.contains("/assistants/") && it.first.endsWith("/prompt") })
    }

    @Test
    fun onlyTheSpokenBlockIsReadAloudAndVoiceModeListensAgain() {
        val store = open()
        enterAndAsk(store, "Which tests are failing")

        store.voice.observe(AppState(events = mapOf("c1" to replied("Which tests are failing", SpokenReply))))

        rule.waitUntil(5_000) { player.played.isNotEmpty() }
        assertEquals(listOf("Two restore tests still fail, both in the snapshot picker."), spokenTexts())
        rule.waitUntil(5_000) { dictation.starts == 2 }
    }

    @Test
    fun withoutASpokenBlockTheWholeReplyIsReadAloud() {
        val store = open()
        enterAndAsk(store, "Is it done")

        store.voice.observe(AppState(events = mapOf("c1" to replied("Is it done", "Yes, both tests pass now."))))

        rule.waitUntil(5_000) { player.played.isNotEmpty() }
        assertEquals(listOf("Yes, both tests pass now."), spokenTexts())
    }

    @Test
    fun theSpokenBlockIsNeverShownInTheThread() {
        val events = """[{"conversation_id":"c1","seq":1,"kind":"user_message","payload":{"text":"Which tests are failing","spoken":true}},{"conversation_id":"c1","seq":2,"kind":"voice_mode","payload":{"on":true}},{"conversation_id":"c1","seq":3,"kind":"acp_update","payload":{"sessionId":"s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":${Json.encodeToString(String.serializer(), SpokenReply)}}}}},{"conversation_id":"c1","seq":4,"kind":"turn_end","payload":{}}]"""
        open(events)

        rule.waitUntil(5_000) { rule.onAllNodesWithText("picker keeps the selection", substring = true).fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Voice mode on").assertExists()
        assertTrue(rule.onAllNodesWithText("snapshot picker", substring = true).fetchSemanticsNodes().isEmpty())
        assertTrue(rule.onAllNodesWithText("mycelium-spoken", substring = true).fetchSemanticsNodes().isEmpty())
    }
}
