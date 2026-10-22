package mycelium

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpStatusCode
import kotlinx.coroutines.test.runTest
import mycelium.api.MyceliumApi
import mycelium.api.QueuedPrompt
import mycelium.api.StreamMessage
import mycelium.state.Store
import mycelium.state.reduce
import kotlin.test.Test
import kotlin.test.assertEquals

class SendingPlaceholderTest {
    @Test
    fun aMessageSentWithTrailingWhitespaceStopsShowingAsSendingOnceTheServerQueuesIt() = runTest {
        val store = Store(MyceliumApi("https://mycelium.test", null, MockEngine { respond("", HttpStatusCode.NoContent) }), backgroundScope)

        store.send("c1", "1p is online\n")
        val queued = QueuedPrompt("q1", "c1", 1, "1p is online", "2026-10-22T10:00:00Z")

        assertEquals(emptyList(), reduce(store.state.value, StreamMessage.QueueChanged("c1", listOf(queued))).sending["c1"])
    }
}
