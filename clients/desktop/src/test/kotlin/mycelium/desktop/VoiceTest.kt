package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.size
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.performImeAction
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
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
import mycelium.api.Event
import mycelium.api.MyceliumApi
import kotlinx.serialization.json.Json
import mycelium.state.Store
import mycelium.state.VoicePhase
import mycelium.state.VoiceProblem
import mycelium.state.AppState
import mycelium.state.VoiceDid
import mycelium.state.VoiceState
import mycelium.state.VoiceTurn
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Shell
import mycelium.ui.VoiceCard
import mycelium.ui.VoiceCardActions
import mycelium.ui.VoiceDockContent
import org.junit.Rule

private const val VoiceOn = """{"voice_enabled":true,"speech_to_text_provider":null,"text_to_speech_provider":null}"""

private const val VoiceOff = """{"voice_enabled":false}"""

private const val VoiceAssistantJson = """{"id":"a-voice","name":"Voice","builtin_key":"voice","created_at":"2026-10-22T10:00:00Z"}"""

private const val VoiceThreadJson = """{"id":"c-voice","owner":"u","node_id":"n-voice","agent":"chat","cwd":"","title":"Voice","status":"working",
"acp_session_id":null,"mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null}"""

private class HeardDictation : Dictation {
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

private class MicrophonePlatform(private val dictation: Dictation?) : Platform by QuietPlatform {
    override fun dictation(): Dictation? = dictation
}

private val NoActions = VoiceCardActions({}, {}, {}, {}, {}, {}, {}, {}, {}, null, {})

class VoiceTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val requests = CopyOnWriteArrayList<Pair<String, String>>()

    private fun open(privacy: String, dictation: Dictation? = HeardDictation(), route: Route = Route.Empty, width: Dp? = null): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            requests += (request.method.value + " " + path) to String(request.body.toByteArray())

            when {
                path.endsWith("/privacy") -> respond(privacy, HttpStatusCode.OK, json)
                path.endsWith("/inbox/feed") -> respond("""{"posts":[]}""", HttpStatusCode.OK, json)
                path.endsWith("/assistants/builtin/voice") -> respond(VoiceAssistantJson, HttpStatusCode.OK, json)
                path.endsWith("/prompt") && request.method == HttpMethod.Post -> respond(VoiceThreadJson, HttpStatusCode.OK, json)
                path.endsWith("/conversations") || path.endsWith("/assistants") || path.endsWith("/nodes") || path.endsWith("/events") -> respond("[]", HttpStatusCode.OK, json)
                else -> respond("{}", HttpStatusCode.OK, json)
            }
        }
        val platform = MicrophonePlatform(dictation)
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create(), platform)

        runBlocking { store.refresh() }
        rule.setContent {
            MyceliumTheme(true) {
                Box(if (width == null) Modifier.fillMaxSize() else Modifier.size(width, 800.dp)) {
                    Shell(store, platform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = route)
                }
            }
        }

        return store
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

        rule.onNodeWithText("Send").assertExists()
        rule.onNodeWithText("Cancel").assertExists()
        rule.onNodeWithContentDescription("Send what was heard").assertExists()
    }

    @Test
    fun stoppingListeningSendsWhatWasHeardAsASpokenMessage() {
        val dictation = HeardDictation()
        open(VoiceOn, dictation)

        rule.onNodeWithContentDescription("Talk to Voice").performClick()
        rule.runOnIdle { dictation.onHeard("open the usage history") }
        rule.onNodeWithText("open the usage history").assertExists()
        rule.onNodeWithText("Send").performClick()
        rule.waitUntil(5_000) { requests.any { it.first.endsWith("/assistants/a-voice/prompt") } }

        val sent = requests.first { it.first.endsWith("/prompt") }.second

        assertTrue(sent.contains("\"text\":\"open the usage history\"") && sent.contains("\"spoken\":true"))
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Thinking").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun theMicrophoneSendsWhatWasHeardWhileListening() {
        val dictation = HeardDictation()
        open(VoiceOn, dictation)

        rule.onNodeWithContentDescription("Talk to Voice").performClick()
        rule.runOnIdle { dictation.onHeard("move the standup to Thursday") }
        rule.onNodeWithContentDescription("Send what was heard").performClick()
        rule.waitUntil(5_000) { requests.any { it.first.endsWith("/assistants/a-voice/prompt") } }

        assertTrue(requests.first { it.first.endsWith("/prompt") }.second.contains("\"text\":\"move the standup to Thursday\""))
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

        rule.onNodeWithText("Set up speech to text").assertExists()
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

    private fun dockShows() = rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Open the Voice thread").fetchSemanticsNodes().isNotEmpty() }

    private fun voiceHeadings() = rule.onAllNodesWithText("Voice").fetchSemanticsNodes().size

    @Test
    fun theDockOnTheInboxDoesNotStartListeningByItself() {
        val dictation = HeardDictation()
        val store = open(VoiceOn, dictation, Route.Inbox)
        dockShows()
        rule.waitForIdle()

        assertTrue(dictation.starts == 0)
        assertTrue(store.voice.state.value.phase == VoicePhase.Idle)
    }

    @Test
    fun theDockMicrophoneStartsListeningOnlyWhenPressed() {
        val dictation = HeardDictation()
        open(VoiceOn, dictation, Route.Inbox)
        dockShows()

        rule.onNodeWithContentDescription("Start listening (Ctrl+Shift+Space)").performClick()

        assertTrue(dictation.starts == 1)
        rule.onNodeWithContentDescription("Stop listening (Ctrl+Shift+Space)").assertExists()
    }

    @Test
    fun theBottomBarMicrophoneOnTheInboxListensInTheDockWithoutTheFloatingCard() {
        val dictation = HeardDictation()
        open(VoiceOn, dictation, Route.Inbox)
        dockShows()

        rule.onNodeWithContentDescription("Talk to Voice").performClick()

        assertTrue(dictation.starts == 1)
        rule.onNodeWithContentDescription("Stop listening (Ctrl+Shift+Space)").assertExists()
        assertTrue(voiceHeadings() == 1)
        assertTrue(rule.onAllNodesWithText("Cancel").fetchSemanticsNodes().isEmpty())
    }

    @Test
    fun closingVoiceFromTheBottomBarLeavesTheIdleDockOnTheInbox() {
        val store = open(VoiceOn, HeardDictation(), Route.Inbox)
        dockShows()
        rule.onNodeWithContentDescription("Talk to Voice").performClick()
        rule.runOnIdle { store.voice.cancelListening() }

        rule.onNodeWithContentDescription("Close Voice").performClick()

        assertTrue(!store.voice.state.value.open)
        rule.onNodeWithContentDescription("Start listening (Ctrl+Shift+Space)").assertExists()
        rule.onNodeWithContentDescription("Talk to Voice").assertExists()
        assertTrue(voiceHeadings() == 1)
    }

    @Test
    fun typingInTheDockWithVoiceOffSendsItToVoice() {
        open(VoiceOff, route = Route.Inbox)
        dockShows()

        rule.onNode(hasSetTextAction()).performTextInput("what needs me")
        rule.onNode(hasSetTextAction()).performImeAction()
        rule.waitUntil(5_000) { requests.any { it.first.endsWith("/assistants/a-voice/prompt") } }

        assertTrue(requests.first { it.first.endsWith("/prompt") }.second.contains("\"text\":\"what needs me\""))
    }

    @Test
    fun theDockMicrophoneWithoutASpeechProviderShowsTheSetupInTheDockAndTypingCarriesOn() {
        open(VoiceOn, dictation = null, route = Route.Inbox)
        dockShows()

        rule.onNodeWithContentDescription("Start listening (Ctrl+Shift+Space)").performClick()

        rule.onNodeWithText("Add a provider with a speech-to-text model in Settings, Providers to dictate on this device.").assertExists()
        rule.onNodeWithText("Set up speech to text").assertExists()
        assertTrue(voiceHeadings() == 1)

        rule.onNodeWithText("Type instead").performClick()

        assertTrue(rule.onAllNodesWithText("Set up speech to text").fetchSemanticsNodes().isEmpty())
        rule.onNodeWithText("Talk or type to Voice").assertExists()
    }

    @Test
    fun awayFromTheInboxVoiceIsTheFloatingCardAlone() {
        open(VoiceOn)

        rule.onNodeWithContentDescription("Talk to Voice").performClick()

        rule.onNodeWithText("Cancel").assertExists()
        assertTrue(voiceHeadings() == 1)
        assertTrue(rule.onAllNodesWithContentDescription("Stop listening (Ctrl+Shift+Space)").fetchSemanticsNodes().isEmpty())
    }

    @Test
    fun voiceStartingOnThePhoneFeedTabReturnsToNeedsYouToShowTheDock() {
        val store = open(VoiceOn, HeardDictation(), Route.Inbox, width = 400.dp)
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Feed").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Feed").performClick()
        rule.waitForIdle()
        assertTrue(rule.onAllNodesWithContentDescription("Open the Voice thread").fetchSemanticsNodes().isEmpty())

        rule.runOnIdle { store.voice.startListening() }

        rule.onNodeWithContentDescription("Stop listening (Ctrl+Shift+Space)").assertExists()
    }

    private fun dock(state: VoiceState, onSay: (String) -> Unit = {}, onOpen: (String) -> Unit = {}) {
        rule.setContent { MyceliumTheme(true) { VoiceDockContent(state, NoActions, onSay, onOpen) } }
    }

    @Test
    fun theDockKeepsEarlierTurnsAndWhatVoiceDid() {
        dock(VoiceState(open = true, phase = VoicePhase.Ended, earlier = listOf(VoiceTurn(said = "Yes, one line", did = listOf(VoiceDid("Answered in Release: Yes, one line", "c1")))), turn = VoiceTurn(said = "Approve", reply = "That one runs a command, so it needs your tap.")))

        rule.onNodeWithText("Yes, one line").assertExists()
        rule.onNodeWithText("Answered in Release: Yes, one line").assertExists()
        rule.onNodeWithText("Approve").assertExists()
        rule.onNodeWithText("That one runs a command, so it needs your tap.").assertExists()
    }

    @Test
    fun typingInTheDockAndSubmittingSaysIt() {
        val said = CopyOnWriteArrayList<String>()
        dock(VoiceState(open = true, phase = VoicePhase.Idle), onSay = { said += it })

        rule.onNode(hasSetTextAction()).performTextInput("Decline")
        rule.onNode(hasSetTextAction()).performImeAction()

        assertTrue(said.single() == "Decline")
    }

    @Test
    fun aLineOfWhatVoiceDidOpensThatConversation() {
        val opened = CopyOnWriteArrayList<String>()
        dock(VoiceState(open = true, phase = VoicePhase.Ended, turn = VoiceTurn(said = "yes", did = listOf(VoiceDid("Answered in Release: yes", "c1")))), onOpen = { opened += it })

        rule.onNodeWithText("Answered in Release: yes").performClick()

        assertTrue(opened.single() == "c1")
    }

    @Test
    fun typingToVoiceSendsItAsAPromptAndARecordedToolBecomesALineOfWhatVoiceDid() {
        val store = open(VoiceOn)

        rule.runOnIdle { store.voice.say("Approve") }
        rule.waitUntil(5_000) { requests.any { it.first.endsWith("/assistants/a-voice/prompt") } }
        rule.waitUntil(5_000) { store.voice.state.value.turn.conversationId == "c-voice" }

        val events = listOf(
            Event("c-voice", 1, "user_message", Json.parseToJsonElement("""{"text":"Approve"}""")),
            Event("c-voice", 2, "acp_update", Json.parseToJsonElement("""{"sessionId":"s","update":{"sessionUpdate":"tool_call","toolCallId":"t1","title":"mcp__mycelium__resolve_needs_you","kind":"other","status":"completed","rawInput":{"key":"c1/flag","action":"approve"},"content":[{"type":"content","content":{"type":"text","text":"Answered in Release: Approve\nNothing else is waiting."}}]}}""")),
            Event("c-voice", 3, "acp_update", Json.parseToJsonElement("""{"sessionId":"s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Done."}}}""")),
            Event("c-voice", 4, "turn_end", Json.parseToJsonElement("""{}""")),
        )
        store.voice.observe(AppState(events = mapOf("c-voice" to events)))

        assertTrue(store.voice.state.value.turn.did == listOf(VoiceDid("Answered in Release: Approve", "c1")))
        assertTrue(store.voice.state.value.phase == VoicePhase.Ended)
        assertTrue(requests.first { it.first.endsWith("/prompt") }.second.contains("\"text\":\"Approve\""))
    }
}
