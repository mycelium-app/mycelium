package mycelium.desktop

import androidx.compose.ui.input.key.Key
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.performMouseInput
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.test.requestFocus
import androidx.compose.ui.test.rightClick
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import java.util.concurrent.CopyOnWriteArrayList
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
import kotlin.test.assertTrue

private object ConsistencyPlatform : Platform {
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

private const val AssistantWithThread = """{"id":"a1","name":"Stand-up","preferences":{},"state":"idle","session_count":1,"conversation_id":"c1","created_at":"2026-10-22T10:00:00Z"}"""

private fun thread(status: String) = """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/c1","title":"Daily summary","status":"$status",
"acp_session_id":null,"mode":null,"model":null,"group_id":null,"assistant_id":"a1","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null,"pinned_at":null}"""

class UiConsistencyTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val posts = CopyOnWriteArrayList<String>()

    private fun show(status: String, route: Route) {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            if (request.method == HttpMethod.Post) {
                posts += path
            }

            val reply = when {
                path.endsWith("/experiments") -> EnabledExperiments
                path.endsWith("/assistants") -> "[$AssistantWithThread]"
                path.endsWith("/conversations") -> "[${thread(status)}]"
                path.endsWith("/groups") || path.endsWith("/projects") || path.endsWith("/nodes") -> "[]"
                path.endsWith("/events") -> "[]"
                else -> "{}"
            }

            respond(reply, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }

        rule.setContent { MyceliumTheme(true) { Shell(store, ConsistencyPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = route) } }
    }

    private fun appears(description: String) = rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription(description).fetchSemanticsNodes().isNotEmpty() }

    private fun assistantRow() = rule.onNode(hasContentDescription("Stand-up, ", substring = true))

    private fun assertAssistantMenuItems() {
        for (item in listOf("Pin", "Close session", "Clear", "Settings", "Archive")) {
            assertEquals(1, rule.onAllNodesWithText(item).fetchSemanticsNodes().size, item)
        }
    }

    @Test
    fun theSidebarAssistantRowMenuClosesItsSession() {
        show("done", Route.Empty)
        rule.waitUntil(5_000) { rule.onAllNodes(hasContentDescription("Stand-up, ", substring = true)).fetchSemanticsNodes().isNotEmpty() }

        assistantRow().performMouseInput { rightClick() }
        assertAssistantMenuItems()

        rule.onNodeWithText("Close session").performClick()

        rule.waitUntil(5_000) { "/api/v1/conversations/c1/close" in posts }
    }

    @Test
    fun shiftF10OnTheFocusedSidebarRowOpensTheSameMenu() {
        show("done", Route.Empty)
        rule.waitUntil(5_000) { rule.onAllNodes(hasContentDescription("Stand-up, ", substring = true)).fetchSemanticsNodes().isNotEmpty() }

        assistantRow().requestFocus()
        assistantRow().performKeyInput {
            keyDown(Key.ShiftLeft)
            pressKey(Key.F10)
            keyUp(Key.ShiftLeft)
        }

        assertAssistantMenuItems()
    }

    @Test
    fun theAssistantThreadHeaderOffersRetryAndDismissWhenTheSessionFailed() {
        show("failed", Route.Assistant("a1"))
        appears("Dismiss the failure of Daily summary")

        assertTrue(rule.onAllNodesWithContentDescription("Retry Daily summary").fetchSemanticsNodes().isNotEmpty())

        rule.onNodeWithContentDescription("Dismiss the failure of Daily summary").performClick()

        rule.waitUntil(5_000) { "/api/v1/conversations/c1/dismiss" in posts }
    }

    @Test
    fun theAssistantThreadHeaderMenuClosesTheSessionAndKeepsTheHeader() {
        show("working", Route.Assistant("a1"))
        appears("More for Daily summary")

        assertTrue(rule.onAllNodesWithText("Stand-up").fetchSemanticsNodes().isNotEmpty())

        rule.onNodeWithContentDescription("More for Daily summary").performClick()
        rule.onNodeWithText("Close session").performClick()

        rule.waitUntil(5_000) { "/api/v1/conversations/c1/close" in posts }
    }
}
