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

private fun assistant() =
    """{"id":"a1","name":"Stand-up","preferences":{},"state":"idle","session_count":0,"created_at":"2026-10-22T10:00:00Z"}"""

private const val Actions = """[
{"id":"x1","assistant_id":"a1","name":"Daily summary","prompt":"Summarise yesterday.","inputs":[],"pinned":true,"position":0},
{"id":"x2","assistant_id":"a1","name":"Meeting notes","prompt":"Notes for {meeting}.","inputs":[{"name":"meeting","kind":"text","required":true}],"pinned":false,"position":1}
]"""

private const val RunConversation =
    """{"id":"s2","owner":"u","node_id":"n1","agent":"chat","cwd":"","title":"Today","status":"review","assistant_id":"a1","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z"}"""

private object ActionsPlatform : Platform {
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

class AssistantActionsTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val requests = java.util.concurrent.CopyOnWriteArrayList<Pair<String, String>>()

    private fun shell() {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            requests += "${request.method.value} $path" to String(request.body.toByteArray())

            val reply = when {
                path.endsWith("/experiments") -> EnabledExperiments
                path.endsWith("/assistants/a1/actions") -> Actions
                path.endsWith("/run") -> RunConversation
                path.endsWith("/assistants") -> "[${assistant()}]"
                path.endsWith("/events") || path.endsWith("/queue") || path.endsWith("/conversations") || path.endsWith("/model-providers") || path.endsWith("/groups") || path.endsWith("/projects") || path.endsWith("/nodes") || path.endsWith("/action-proposals") -> "[]"
                else -> "{}"
            }

            respond(reply, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }

        rule.setContent { MyceliumTheme(true) { Shell(store, ActionsPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Assistant("a1")) } }
    }

    @Test
    fun aPinnedActionWithoutInputsRunsFromItsChipAtOnce() {
        shell()

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Run Daily summary").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Run Daily summary").performClick()

        rule.waitUntil(5_000) { requests.any { it.first == "POST /api/v1/assistants/a1/actions/x1/run" } }
        assertEquals("{}", requests.last { it.first.endsWith("/x1/run") }.second)
    }

    @Test
    fun anActionWithInputsAsksForThemFromTheActionsMenuBeforeRunning() {
        shell()

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Actions").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Actions").performClick()
        rule.onNodeWithText("Meeting notes").performClick()

        rule.onNodeWithText("Run").performClick()
        assertEquals(0, requests.count { it.first.endsWith("/x2/run") })
        rule.onNodeWithText("Enter a value for meeting.").assertExists()

        rule.onNodeWithContentDescription("Meeting", substring = true).performTextInput("Planning")
        rule.onNodeWithText("Run").performClick()

        rule.waitUntil(5_000) { requests.any { it.first == "POST /api/v1/assistants/a1/actions/x2/run" } }
        assertEquals("""{"inputs":{"meeting":"Planning"}}""", requests.last { it.first.endsWith("/x2/run") }.second)
    }
}
