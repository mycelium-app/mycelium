package mycelium.desktop

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.performClick
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import java.util.concurrent.CopyOnWriteArrayList
import kotlinx.coroutines.runBlocking
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Shell
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private const val FailedJson = """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/app","title":"Plan the rollout","status":"failed",
"acp_session_id":"s1","mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null}"""

private const val LimitError = """[{"conversation_id":"c1","seq":1,"kind":"error","payload":{"message":"This node already runs 4 conversations, its limit."}}]"""

class RetryTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val retried = CopyOnWriteArrayList<String>()

    private fun store(events: String = LimitError): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            if (request.method == HttpMethod.Post && path.endsWith("/retry")) {
                retried += path

                return@MockEngine respond("", HttpStatusCode.NoContent)
            }

            val body = when {
                path.endsWith("/conversations") -> "[$FailedJson]"
                path.endsWith("/conversations/c1") -> FailedJson
                path.endsWith("/events") -> events
                path.endsWith("/queue") || path.endsWith("/nodes") || path.endsWith("/groups") || path.endsWith("/projects") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }

        return store
    }

    private fun show(store: Store) {
        rule.setContent { MyceliumTheme(true) { Shell(store, QuietPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Chat("c1")) } }
    }

    @Test
    fun aFailedConversationHasOneRetryAndItIsInTheError() {
        show(store())

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Retry", substring = true).fetchSemanticsNodes().isNotEmpty() }

        assertEquals(1, rule.onAllNodesWithContentDescription("Retry", substring = true).fetchSemanticsNodes().size)
        assertEquals(0, rule.onAllNodesWithContentDescription("Retry Plan the rollout").fetchSemanticsNodes().size)
        rule.onNodeWithContentDescription("Error: This node already runs 4 conversations, its limit.").assertExists()
    }

    @Test
    fun aFailedConversationWithNoErrorEventStillHasOneRetry() {
        show(store(events = "[]"))

        rule.onNodeWithContentDescription("Error: The agent stopped with an error.").assertExists()
        assertEquals(1, rule.onAllNodesWithContentDescription("Retry", substring = true).fetchSemanticsNodes().size)

        rule.onNodeWithContentDescription("Retry this conversation").performClick()
        rule.waitUntil(5_000) { retried.isNotEmpty() }
    }

    @Test
    fun retryInTheErrorRetriesTheConversation() {
        show(store())

        rule.onNodeWithContentDescription("Error: This node already runs 4 conversations, its limit.").assertExists()
        rule.onNodeWithContentDescription("Retry this conversation").performClick()
        rule.waitUntil(5_000) { retried.isNotEmpty() }

        assertEquals(listOf("/api/v1/conversations/c1/retry"), retried)
    }
}
