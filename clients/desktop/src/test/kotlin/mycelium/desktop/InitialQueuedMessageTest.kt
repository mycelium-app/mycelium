package mycelium.desktop

import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.runBlocking
import mycelium.api.Event
import mycelium.api.StreamMessage
import kotlinx.serialization.json.Json
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.ConversationScreen
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private const val StartingConversation = """{"id":"c1","owner":"u","node_id":"n1","agent":"codex","cwd":"/work/app","title":"New workspace","status":"working","created_at":"2026-10-24T10:00:00Z","updated_at":"2026-10-24T10:00:00Z"}"""
private const val InitialQueue = """[{"id":"q1","conversation_id":"c1","position":1,"text":"Build the search screen","created_at":"2026-10-24T10:00:00Z"}]"""

class InitialQueuedMessageTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    @Test
    fun aStartingConversationsFirstMessageAppearsInTheThreadWhileWaitingForTheAgent() {
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val body = when {
                path.endsWith("/conversations") -> "[$StartingConversation]"
                path.endsWith("/conversations/c1") -> StartingConversation
                path.endsWith("/queue") -> InitialQueue
                path.endsWith("/events") || path.endsWith("/nodes") || path.endsWith("/artifacts") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json"))
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh(); store.open("c1") }
        rule.setContent {
            MyceliumTheme(true) {
                val state by store.state.collectAsState()
                ConversationScreen(store, state, "c1", onArchived = {}, panels = false)
            }
        }

        rule.onNodeWithText("Build the search screen").assertIsDisplayed()
        assertEquals(0, rule.onAllNodesWithText("1 queued message").fetchSemanticsNodes().size)
        assertEquals(1, rule.onAllNodesWithText("Build the search screen").fetchSemanticsNodes().size)

        rule.runOnIdle {
            store.receive(StreamMessage.EventAppended(Event("c1", 1, "user_message", Json.parseToJsonElement("""{"text":"Build the search screen","prompt_id":"q1"}"""))))
        }

        assertEquals(0, rule.onAllNodesWithText("1 queued message").fetchSemanticsNodes().size)
        assertEquals(1, rule.onAllNodesWithText("Build the search screen").fetchSemanticsNodes().size)
    }
}
