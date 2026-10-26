package mycelium.desktop

import androidx.compose.ui.test.assertIsDisplayed
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

private const val Nodes = """[{"id":"n1","name":"willow","hostname":"willow","os":"linux","arch":"x86_64","version":"v0.11.0","address":"10.0.0.5","online":true,"draining":false,"sessions":["c1"],
"agents":[{"agent":"claude","available":true},{"agent":"codex","available":true}],
"logins":[{"agent":"claude","account":"sam@example.com","method":"token","state":"signed_in"},{"agent":"claude","account":"work@example.org","method":"token","state":"signed_in"},{"agent":"codex","account":"codex@example.com","method":"token","state":"signed_in"}]}]"""

private const val Started = """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/app","title":"Backups","status":"done","acp_session_id":"s","account":"sam@example.com","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z"}"""

private const val Continued = """[{"conversation_id":"c1","seq":1,"kind":"continued","payload":{"kind":"handoff","agent":"codex","account":"codex@example.com","from":{"agent":"claude","account":"sam@example.com"}},"created_at":"2026-10-22T10:00:00Z"}]"""

private object ContinuePlatform : Platform {
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

class ContinueOnTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val requests = java.util.Collections.synchronizedList(mutableListOf<Pair<String, String>>())

    private fun shell(experiments: String, events: String = "[]") {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val reply = when {
                path.endsWith("/experiments") -> experiments
                path.endsWith("/continue") -> Started.replace("claude", "codex")
                path.endsWith("/nodes") -> Nodes
                path.endsWith("/conversations") -> "[$Started]"
                path.endsWith("/conversations/c1") -> Started
                path.endsWith("/events") -> events
                path.endsWith("/queue") || path.endsWith("/model-providers") -> "[]"
                else -> "{}"
            }

            requests += "${request.method.value} $path" to String(request.body.toByteArray())

            respond(reply, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }

        rule.setContent { MyceliumTheme(true) { Shell(store, ContinuePlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Chat("c1")) } }
    }

    @Test
    fun continuingOnTheOtherHarnessSendsTheRequest() {
        shell(EnabledExperiments)

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Continue on…: sam@example.com").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Continue on…: sam@example.com").performClick()
        rule.onNodeWithText("Codex · codex@example.com").performClick()

        rule.waitUntil(5_000) { requests.any { it.first == "POST /api/v1/conversations/c1/continue" } }
        assertEquals("""{"agent":"codex","account":"codex@example.com"}""", requests.last { it.first == "POST /api/v1/conversations/c1/continue" }.second)
    }

    @Test
    fun aContinuedEventIsShownInTheThreadAsADivider() {
        shell(EnabledExperiments, Continued)

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Continued on Codex as codex@example.com").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Continued on Codex as codex@example.com").assertIsDisplayed()
    }

    @Test
    fun withTheExperimentOffTheAccountStaysFixedAndThereIsNoContinueButton() {
        shell(EnabledExperiments.replace("\"continue_elsewhere\":true", "\"continue_elsewhere\":false"))

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Account: sam@example.com, fixed once the conversation has started").fetchSemanticsNodes().isNotEmpty() }

        assertEquals(0, rule.onAllNodesWithContentDescription("Continue on…: sam@example.com").fetchSemanticsNodes().size)
    }
}
