package mycelium

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.withTimeout
import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.api.MyceliumApi
import mycelium.state.Store
import kotlin.test.Test
import kotlin.test.assertEquals

class ConversationTitleTest {
    private val conversation = Conversation("c1", "u", "n", "claude", "/work", "Mine", ConversationStatus.Done, createdAt = "2026-10-22T10:00:00Z", updatedAt = "2026-10-22T10:00:00Z", titleUserSet = true)

    private fun storeAnswering(body: String, sent: MutableStateFlow<List<String>>, scope: kotlinx.coroutines.CoroutineScope): Store {
        val engine = MockEngine { request ->
            sent.update { it + String(request.body.toByteArray()) }

            respond(body, HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json"))
        }

        return Store(MyceliumApi("https://mycelium.test", null, engine), scope)
    }

    private val predicted = """{"id":"c1","owner":"u","node_id":"n","agent":"claude","cwd":"/work","title":"Share the restore strings","title_user_set":false,"status":"done","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z"}"""

    @Test
    fun savingAnEmptyTitleAsksTheServerForTheSuggestedOneAndShowsIt() = runTest {
        val sent = MutableStateFlow(emptyList<String>())
        val store = storeAnswering(predicted, sent, backgroundScope)

        store.rename(conversation, "")
        withTimeout(5_000) { while (store.state.value.conversations["c1"]?.title != "Share the restore strings") delay(10) }

        assertEquals(listOf("""{"clear_title":true}"""), sent.value)
        assertEquals(false, store.state.value.conversations["c1"]?.titleUserSet)
    }

    @Test
    fun savingATitleSendsItAndMarksTheTitleAsTheUsers() = runTest {
        val sent = MutableStateFlow(emptyList<String>())
        val store = storeAnswering(predicted.replace("Share the restore strings", "Restore work").replace("\"title_user_set\":false", "\"title_user_set\":true"), sent, backgroundScope)

        store.rename(conversation.copy(title = "Old", titleUserSet = false), "Restore work")

        assertEquals(true, store.state.value.conversations["c1"]?.titleUserSet)
        withTimeout(5_000) { while (sent.value.isEmpty()) delay(10) }
        assertEquals(listOf("""{"title":"Restore work"}"""), sent.value)
    }
}
