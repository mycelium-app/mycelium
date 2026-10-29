package mycelium.desktop

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import java.nio.file.Path
import java.util.concurrent.CopyOnWriteArrayList
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue
import kotlinx.coroutines.runBlocking
import mycelium.Dictation
import mycelium.DictationProblem
import mycelium.Platform
import mycelium.WakeListener
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.state.VoicePhase
import mycelium.ui.MyceliumTheme
import mycelium.ui.PrivacySettings
import mycelium.ui.Route
import mycelium.ui.Shell
import org.junit.Assume
import org.junit.Rule

private const val VoiceOn = """{"voice_enabled":true,"speech_to_text_provider":null,"text_to_speech_provider":null}"""

private const val VoiceOff = """{"voice_enabled":false}"""

private class FakeWakeListener(private val events: MutableList<String>) : WakeListener {
    var phrase: String? = null
    var onHeard: () -> Unit = {}
    var microphoneAvailable = true

    override fun start(phrase: String, onHeard: () -> Unit): Boolean {
        events += "wake started for $phrase"
        this.phrase = phrase
        this.onHeard = onHeard

        return microphoneAvailable
    }

    override fun stop() {
        events += "wake stopped"
    }
}

private class RecordingDictation(private val events: MutableList<String>) : Dictation {
    override fun start(onHeard: (String) -> Unit, onStopped: (DictationProblem?) -> Unit) {
        events += "dictation started"
    }

    override fun stop() = Unit
}

private class WakePlatform(
    private val listener: WakeListener?,
    private val dictation: Dictation,
    private val saved: Map<String, String>,
) : Platform by QuietPlatform {
    override fun wakeListener(): WakeListener? = listener

    override val hearsWakePhrase: Boolean get() = listener != null

    override fun dictation(): Dictation = dictation

    override fun loadPreference(key: String): String? = saved[key]
}

class WakePhraseTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val events = CopyOnWriteArrayList<String>()
    private val listener = FakeWakeListener(events)

    private fun store(privacy: String, saved: Map<String, String> = emptyMap(), hasListener: Boolean = true): Pair<Store, Platform> {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { respond(privacy, HttpStatusCode.OK, json) }
        val platform = WakePlatform(listener.takeIf { hasListener }, RecordingDictation(events), saved)
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create(), platform)

        runBlocking { store.voice.load() }

        return store to platform
    }

    private fun waitingStatuses() = rule.onAllNodesWithText("Waiting for", substring = true).fetchSemanticsNodes().size

    private fun settings(privacy: String, saved: Map<String, String> = emptyMap(), hasListener: Boolean = true) {
        val (store, platform) = store(privacy, saved, hasListener)

        rule.setContent { MyceliumTheme(true) { PrivacySettings(store, platform) } }
    }

    @Test
    fun turningOnTheWakePhraseWaitsForHeyMycelium() {
        settings(VoiceOn)

        rule.onNodeWithContentDescription("Listen for a wake phrase").performClick()

        rule.onNodeWithText("Waiting for \"Hey Mycelium\". Say it, then what you want.").assertExists()
        assertEquals("hey_mycelium", listener.phrase)
    }

    @Test
    fun choosingOkMyceliumWaitsForThatPhraseInstead() {
        settings(VoiceOn, mapOf("voice-wake-phrase-enabled" to "true"))

        rule.onNodeWithText("OK Mycelium").performClick()

        rule.onNodeWithText("Waiting for \"OK Mycelium\". Say it, then what you want.").assertExists()
        assertEquals("ok_mycelium", listener.phrase)
    }

    @Test
    fun turningTheWakePhraseOffStopsWaiting() {
        settings(VoiceOn, mapOf("voice-wake-phrase-enabled" to "true"))

        rule.onNodeWithContentDescription("Listen for a wake phrase").performClick()
        rule.waitForIdle()

        assertEquals(0, waitingStatuses())
        assertEquals("wake stopped", events.last())
    }

    @Test
    fun withoutAMicrophoneTheSettingsDoNotClaimToBeWaiting() {
        listener.microphoneAvailable = false
        settings(VoiceOn, mapOf("voice-wake-phrase-enabled" to "true"))

        assertEquals(0, waitingStatuses())
    }

    @Test
    fun withoutAWakeListenerTheSettingsSayItIsNotAvailable() {
        settings(VoiceOn, hasListener = false)

        rule.onNodeWithText("The wake phrase is not available on this device.").assertExists()
    }

    @Test
    fun theMicrophoneNamesTheWakePhraseWhileWaiting() {
        val (store, platform) = store(VoiceOn, mapOf("voice-wake-phrase-enabled" to "true"))

        rule.setContent { MyceliumTheme(true) { Shell(store, platform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Empty) } }

        rule.onNodeWithContentDescription("Talk to Voice, listening for Hey Mycelium").assertExists()
    }

    @Test
    fun hearingTheWakePhraseStopsTheWakeListenerBeforeListeningStarts() {
        val (store, _) = store(VoiceOn, mapOf("voice-wake-phrase-enabled" to "true"))

        listener.onHeard()

        assertEquals(VoicePhase.Listening, store.voice.state.value.phase)
        assertTrue(events.indexOf("wake stopped") < events.indexOf("dictation started"))
        assertFalse(store.voice.state.value.waitingForWakePhrase)
    }

    @Test
    fun waitingResumesWhenListeningEnds() {
        val (store, _) = store(VoiceOn, mapOf("voice-wake-phrase-enabled" to "true"))

        listener.onHeard()
        store.voice.cancelListening()

        assertTrue(store.voice.state.value.waitingForWakePhrase)
        assertEquals("wake started for hey_mycelium", events.last())
    }

    @Test
    fun voiceTurnedOffNeverWaitsForTheWakePhrase() {
        val (store, _) = store(VoiceOff, mapOf("voice-wake-phrase-enabled" to "true"))

        assertFalse(store.voice.state.value.waitingForWakePhrase)
        assertNull(listener.phrase)
    }

    @Test
    fun theDownloadedModelHearsNothingInSilence() {
        val directory = System.getProperty("mycelium.wakeModel")

        Assume.assumeTrue(directory != null)

        val spotter = openWakeSpotter(Path.of(directory))
        val stream = spotter.createStream()

        stream.acceptWaveform(FloatArray(32000), 16000)
        stream.inputFinished()

        val keywords = mutableListOf<String>()

        while (spotter.isReady(stream)) {
            spotter.decode(stream)
            keywords += spotter.getResult(stream).keyword
        }

        stream.release()
        spotter.release()

        assertTrue(keywords.all { it.isEmpty() })
    }
}
