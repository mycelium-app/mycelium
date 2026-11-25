package mycelium.desktop

import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isHeading
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.performClick
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
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
import kotlin.test.assertTrue

private const val FailedJson = """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/app","title":"Plan the rollout","status":"failed",
"acp_session_id":"s1","account":"sam@example.com","mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null}"""

private const val SignInError = """[{"conversation_id":"c1","seq":1,"kind":"error","payload":{"message":"Authentication required","sign_in_required":true}}]"""

private const val ExpiredOnWillow = """[{"id":"n1","name":"willow","hostname":"willow","os":"linux","arch":"x86_64","version":"0.1.0","online":true,"draining":false,
 "agents":[{"agent":"claude","version":"2.1.308","available":true}],
 "logins":[{"agent":"claude","account":"sam@example.com","method":"oauth_token","state":"expired"}]}]"""

private const val LimitError = """[{"conversation_id":"c1","seq":1,"kind":"error","payload":{"message":"This node already runs 4 conversations, its limit."}}]"""

class RetryTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val retried = CopyOnWriteArrayList<String>()
    private val logins = CopyOnWriteArrayList<String>()

    private fun store(events: String = LimitError, conversation: String = FailedJson, nodes: String = "[]"): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            if (request.method == HttpMethod.Post && path.endsWith("/retry")) {
                retried += path

                return@MockEngine respond("", HttpStatusCode.NoContent)
            }

            if (request.method == HttpMethod.Post && path.endsWith("/logins")) {
                logins += path + " " + String(request.body.toByteArray())

                return@MockEngine respond("""{"login_id":"l1"}""", HttpStatusCode.OK, json)
            }

            val body = when {
                path.endsWith("/conversations") -> "[$conversation]"
                path.endsWith("/conversations/c1") -> conversation
                path.endsWith("/events") -> events
                path.endsWith("/nodes") -> nodes
                path.endsWith("/queue") || path.endsWith("/groups") || path.endsWith("/projects") -> "[]"
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

    @Test
    fun aFailureThatNeedsASignInStartsTheSignInForThatAccountInsteadOfRetrying() {
        show(store(SignInError))

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Sign in Claude Code again").fetchSemanticsNodes().isNotEmpty() }
        assertEquals(0, rule.onAllNodesWithContentDescription("Retry", substring = true).fetchSemanticsNodes().size)

        rule.onNodeWithContentDescription("Sign in Claude Code again").performClick()
        rule.waitUntil(5_000) { logins.isNotEmpty() }

        assertTrue(logins.single().startsWith("/api/v1/nodes/n1/logins "), logins.single())
        assertTrue(logins.single().contains("\"account\":\"sam@example.com\""), logins.single())
        assertEquals(emptyList(), retried)
    }

    @Test
    fun aSignInCardNamesTheAccountAndNodeAndItsSignInSaysExactlyWhatItSignsIn() {
        show(store(SignInError, nodes = ExpiredOnWillow))

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Error: Authentication required. sam@example.com on willow").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Sign in Claude Code again").performClick()

        rule.waitUntil(5_000) { rule.onAllNodes(hasText("Sign in to Claude Code as sam@example.com on willow") and isHeading()).fetchSemanticsNodes().isNotEmpty() }
        assertEquals(0, rule.onAllNodesWithText("This node's default sign-in has expired.", substring = true).fetchSemanticsNodes().size)
    }

    @Test
    fun aChatOnTheDefaultSignInSaysSoAndNamesWhoWasLastSignedIn() {
        show(store(SignInError, FailedJson.replace("\"account\":\"sam@example.com\"", "\"account\":null"), ExpiredOnWillow))

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Error: Authentication required. Default sign-in on willow").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Sign in Claude Code again").performClick()

        rule.waitUntil(5_000) { rule.onAllNodes(hasText("Sign in to Claude Code on willow") and isHeading()).fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("This node's default sign-in has expired. Sign in with the account this chat should use.").assertExists()
        rule.onNodeWithText("Last signed in as sam@example.com").assertExists()
        assertTrue(logins.single().startsWith("/api/v1/nodes/n1/logins "), logins.single())
    }
}
