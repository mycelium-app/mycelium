package mycelium.desktop

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.runBlocking
import mycelium.Connection
import mycelium.DeviceInfo
import mycelium.Platform
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Shell
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private fun session(id: String, title: String) =
    """{"id":"$id","owner":"u","node_id":"n1","agent":"chat","cwd":"","title":"$title","status":"review","assistant_id":"a1","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z"}"""

private fun assistant(session: String?) =
    """{"id":"a1","name":"Stand-up","preferences":{},"state":"idle","session_count":2,${session?.let { "\"conversation_id\":\"$it\"," }.orEmpty()}"secrets":[{"name":"JELLYFIN_KEY","description":"From the dashboard","state":"waiting"}],"created_at":"2026-10-22T10:00:00Z"}"""

private fun said(conversation: String, text: String) =
    """[{"conversation_id":"$conversation","seq":1,"kind":"user_message","payload":{"text":"$text"},"created_at":"2026-10-22T10:00:00Z"}]"""

private object ThreadPlatform : Platform {
    override val originUrl: String? = null
    override val signsInThroughTheBrowser = false
    override fun loadConnection(): Connection? = null
    override fun saveConnection(connection: Connection?) = Unit
    override fun device() = DeviceInfo("Desktop", "host", "linux", "x86_64")
    override fun approveCode(): String? = null
    override fun currentPath() = "/"
    override fun navigate(url: String) = Unit
    override fun openUrl(url: String) = Unit
    override fun copyToClipboard(text: String) = Unit
}

class AssistantThreadTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val requests = java.util.concurrent.CopyOnWriteArrayList<Pair<String, String>>()

    private fun shell(current: String?, withEarlier: Boolean = true) {
        var held = assistant(current)
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val sessions = listOfNotNull(session("s2", "Today").takeIf { current != null }, session("s1", "Yesterday").takeIf { withEarlier })
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            requests += "${request.method.value} $path" to String(request.body.toByteArray())

            val reply = when {
                path.endsWith("/experiments") -> EnabledExperiments
                path.endsWith("/assistants") -> "[$held]"
                path.endsWith("/secrets/JELLYFIN_KEY") -> """{"name":"JELLYFIN_KEY","state":"set","set_by":"person"}"""
                path.endsWith("/assistants/a1/clear") -> assistant(null).also { held = it }
                path.endsWith("/assistants/a1/prompt") && request.method == HttpMethod.Post -> session("s2", "Today").also { held = assistant("s2") }
                path.endsWith("/conversations") -> sessions.joinToString(prefix = "[", postfix = "]")
                path.endsWith("/conversations/s2/events") -> said("s2", "Good morning")
                path.endsWith("/conversations/s1/events") -> said("s1", "Old question")
                path.endsWith("/conversations/s2") -> session("s2", "Today")
                path.endsWith("/events") || path.endsWith("/queue") || path.endsWith("/model-providers") || path.endsWith("/groups") || path.endsWith("/projects") || path.endsWith("/nodes") -> "[]"
                else -> "{}"
            }

            respond(reply, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }

        rule.setContent { MyceliumTheme(true) { Shell(store, ThreadPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Assistant("a1")) } }
    }

    private fun present(text: String) = rule.onAllNodesWithText(text).fetchSemanticsNodes().size

    @Test
    fun sendingToAnAssistantShowsTheMessageAtOnceAndClearEmptiesTheThread() {
        shell(current = null, withEarlier = false)

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Message").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Message").performTextInput("Good morning")
        rule.onNodeWithContentDescription("Send").performClick()

        rule.waitUntil(5_000) { requests.any { it.first == "POST /api/v1/assistants/a1/prompt" } }
        assertEquals("""{"text":"Good morning"}""", requests.last { it.first == "POST /api/v1/assistants/a1/prompt" }.second)
        rule.waitUntil(5_000) { present("Good morning") > 0 }

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("More for Today").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("More for Today").performClick()
        rule.onNodeWithText("Clear").performClick()

        rule.waitUntil(5_000) { requests.any { it.first == "POST /api/v1/assistants/a1/clear" } }
        rule.waitUntil(5_000) { present("Good morning") == 0 }
    }

    @Test
    fun earlierSessionsExpandFromTheDivider() {
        shell(current = "s2")

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("1 earlier session, collapsed").fetchSemanticsNodes().isNotEmpty() }
        assertEquals(0, present("Old question"))

        rule.onNodeWithContentDescription("1 earlier session, collapsed").performClick()

        rule.waitUntil(5_000) { present("Old question") > 0 }
    }

    @Test
    fun aWaitingSecretCardSetsTheSecretWithoutShowingItsValue() {
        shell(current = null, withEarlier = false)

        rule.waitUntil(5_000) { rule.onAllNodesWithText("needs JELLYFIN_KEY", substring = true).fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Set secret JELLYFIN_KEY").performClick()
        rule.onNodeWithContentDescription("Value of JELLYFIN_KEY").performTextInput("abc123def")
        rule.onNodeWithContentDescription("Save secret JELLYFIN_KEY").performClick()

        rule.waitUntil(5_000) { requests.any { it.first == "PUT /api/v1/assistants/a1/secrets/JELLYFIN_KEY" } }
        assertEquals("""{"value":"abc123def"}""", requests.last { it.first.startsWith("PUT") }.second)
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Save secret JELLYFIN_KEY").fetchSemanticsNodes().isEmpty() }
        assertEquals(0, present("abc123def"))
    }
}
