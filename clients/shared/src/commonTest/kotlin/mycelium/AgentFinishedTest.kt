package mycelium

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpStatusCode
import kotlinx.coroutines.test.runTest
import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.api.MyceliumApi
import mycelium.api.QueuedPrompt
import mycelium.api.StreamMessage
import mycelium.state.NotifyFinishedPreference
import mycelium.state.NotifySoundPreference
import mycelium.state.Store
import kotlin.test.Test
import kotlin.test.assertEquals

private class RecordingPlatform : Platform {
    val calls = mutableListOf<String>()
    val preferences = mutableMapOf<String, String>()
    var focused = false

    override val signsInThroughTheBrowser = false
    override val originUrl: String? = null

    override fun loadConnection(): Connection? = null
    override fun saveConnection(connection: Connection?) = Unit
    override fun device() = DeviceInfo("test", "test", "linux", "x86_64")
    override fun approveCode(): String? = null
    override fun currentPath() = "/"
    override fun navigate(url: String) = Unit
    override fun openUrl(url: String) = Unit
    override fun copyToClipboard(text: String) = Unit

    override fun loadPreference(key: String): String? = preferences[key]
    override fun windowFocused() = focused

    override fun notify(title: String, body: String, conversationId: String, sound: Boolean) {
        calls += "notify $title | $body | $conversationId"
    }

    override fun chime() {
        calls += "chime"
    }
}

private fun conversation(id: String, status: ConversationStatus, sideKind: String? = null) =
    Conversation(id, "u", "n1", "claude", "/work", "Fix login $id", status, createdAt = "2026-10-23T10:00:00Z", updatedAt = "2026-10-23T10:00:00Z", sideKind = sideKind)

private fun storeWith(platform: Platform, scope: kotlinx.coroutines.CoroutineScope) =
    Store(MyceliumApi("https://mycelium.test", null, MockEngine { respond("", HttpStatusCode.NoContent) }), scope, platform)

class AgentFinishedTest {
    @Test
    fun workingToDoneNotifiesAndChimes() = runTest {
        val platform = RecordingPlatform()
        val store = storeWith(platform, backgroundScope)

        store.receive(StreamMessage.ConversationChanged(conversation("c1", ConversationStatus.Working)))
        store.receive(StreamMessage.ConversationChanged(conversation("c1", ConversationStatus.Done)))

        assertEquals(listOf("notify Fix login c1 | Done | c1", "chime"), platform.calls)
    }

    @Test
    fun eachOutcomeHasItsOwnWording() = runTest {
        val wording = mapOf(
            ConversationStatus.Input to "Needs your input",
            ConversationStatus.Review to "Ready to review",
            ConversationStatus.Failed to "Failed",
        )

        for ((status, body) in wording) {
            val platform = RecordingPlatform()
            val store = storeWith(platform, backgroundScope)

            store.receive(StreamMessage.ConversationChanged(conversation("c1", ConversationStatus.Working)))
            store.receive(StreamMessage.ConversationChanged(conversation("c1", status)))

            assertEquals("notify Fix login c1 | $body | c1", platform.calls.first())
        }
    }

    @Test
    fun doneToDoneDoesNotNotify() = runTest {
        val platform = RecordingPlatform()
        val store = storeWith(platform, backgroundScope)

        store.receive(StreamMessage.ConversationChanged(conversation("c1", ConversationStatus.Done)))
        store.receive(StreamMessage.ConversationChanged(conversation("c1", ConversationStatus.Done)))

        assertEquals(emptyList(), platform.calls)
    }

    @Test
    fun aConversationFirstSeenAlreadyFinishedDoesNotNotify() = runTest {
        val platform = RecordingPlatform()
        val store = storeWith(platform, backgroundScope)

        store.receive(StreamMessage.ConversationChanged(conversation("c1", ConversationStatus.Done)))

        assertEquals(emptyList(), platform.calls)
    }

    @Test
    fun theOpenConversationInAFocusedWindowDoesNotNotify() = runTest {
        val platform = RecordingPlatform().also { it.focused = true }
        val store = storeWith(platform, backgroundScope)
        store.viewing = "c1"

        store.receive(StreamMessage.ConversationChanged(conversation("c1", ConversationStatus.Working)))
        store.receive(StreamMessage.ConversationChanged(conversation("c1", ConversationStatus.Done)))

        assertEquals(emptyList(), platform.calls)
    }

    @Test
    fun theOpenConversationInAnUnfocusedWindowStillNotifies() = runTest {
        val platform = RecordingPlatform()
        val store = storeWith(platform, backgroundScope)
        store.viewing = "c1"

        store.receive(StreamMessage.ConversationChanged(conversation("c1", ConversationStatus.Working)))
        store.receive(StreamMessage.ConversationChanged(conversation("c1", ConversationStatus.Done)))

        assertEquals(2, platform.calls.size)
    }

    @Test
    fun anotherConversationNotifiesWhileOneIsOpenAndFocused() = runTest {
        val platform = RecordingPlatform().also { it.focused = true }
        val store = storeWith(platform, backgroundScope)
        store.viewing = "c1"

        store.receive(StreamMessage.ConversationChanged(conversation("c2", ConversationStatus.Working)))
        store.receive(StreamMessage.ConversationChanged(conversation("c2", ConversationStatus.Done)))

        assertEquals(2, platform.calls.size)
    }

    @Test
    fun turningTheSoundOffKeepsTheNotification() = runTest {
        val platform = RecordingPlatform().also { it.preferences[NotifySoundPreference] = "false" }
        val store = storeWith(platform, backgroundScope)

        store.receive(StreamMessage.ConversationChanged(conversation("c1", ConversationStatus.Working)))
        store.receive(StreamMessage.ConversationChanged(conversation("c1", ConversationStatus.Done)))

        assertEquals(listOf("notify Fix login c1 | Done | c1"), platform.calls)
    }

    @Test
    fun turningNotificationsOffKeepsTheChime() = runTest {
        val platform = RecordingPlatform().also { it.preferences[NotifyFinishedPreference] = "false" }
        val store = storeWith(platform, backgroundScope)

        store.receive(StreamMessage.ConversationChanged(conversation("c1", ConversationStatus.Working)))
        store.receive(StreamMessage.ConversationChanged(conversation("c1", ConversationStatus.Done)))

        assertEquals(listOf("chime"), platform.calls)
    }

    @Test
    fun aSideChatFinishingDoesNotNotify() = runTest {
        val platform = RecordingPlatform()
        val store = storeWith(platform, backgroundScope)

        store.receive(StreamMessage.ConversationChanged(conversation("s1", ConversationStatus.Working, "side")))
        store.receive(StreamMessage.ConversationChanged(conversation("s1", ConversationStatus.Done, "side")))

        assertEquals(emptyList(), platform.calls)
    }

    @Test
    fun aTurnFinishingWithAPromptQueuedDoesNotNotify() = runTest {
        val platform = RecordingPlatform()
        val store = storeWith(platform, backgroundScope)

        store.receive(StreamMessage.ConversationChanged(conversation("c1", ConversationStatus.Working)))
        store.receive(StreamMessage.QueueChanged("c1", listOf(QueuedPrompt("q1", "c1", 0, "and the tests", "2026-10-23T10:01:00Z"))))
        store.receive(StreamMessage.ConversationChanged(conversation("c1", ConversationStatus.Done)))

        assertEquals(emptyList(), platform.calls)
    }
}
