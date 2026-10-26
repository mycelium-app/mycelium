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
import kotlin.test.assertTrue

private fun assistantJson(name: String, pinned: Boolean = false, description: String? = "Daily summary") =
    """{"id":"a-$name","name":"$name",${description?.let { "\"description\":\"$it\"," }.orEmpty()}"preferences":{},"state":"idle","session_count":0,${if (pinned) "\"pinned_at\":\"2026-10-22T10:00:00Z\"," else ""}"created_at":"2026-10-22T10:00:00Z"}"""

private object AssistantsPlatform : Platform {
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

class AssistantsTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val requests = java.util.concurrent.CopyOnWriteArrayList<Pair<String, String>>()

    private fun shell(assistants: MutableList<String>, route: Route = Route.Empty, experiments: String = EnabledExperiments) {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val body = String(request.body.toByteArray())

            requests += "${request.method.value} $path" to body

            val reply = when {
                path.endsWith("/experiments") -> experiments
                path.endsWith("/assistants") && request.method == HttpMethod.Post -> assistantJson(Regex("\"name\":\"([^\"]*)\"").find(body)!!.groupValues[1]).also { assistants += it }
                path.endsWith("/assistants") -> assistants.joinToString(prefix = "[", postfix = "]")
                path.contains("/assistants/") && request.method == HttpMethod.Patch -> assistantJson(path.substringAfterLast("/a-"), pinned = true).also { changed -> assistants.replaceAll { if (it.contains(path.substringAfterLast("/"))) changed else it } }
                path.endsWith("/groups") || path.endsWith("/projects") || path.endsWith("/nodes") || path.endsWith("/conversations") -> "[]"
                else -> "{}"
            }

            respond(reply, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }

        rule.setContent { MyceliumTheme(true) { Shell(store, AssistantsPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = route) } }
    }

    private fun present(text: String) = rule.onAllNodesWithText(text).fetchSemanticsNodes().size

    @Test
    fun turningOnTheExperimentShowsAssistantsInTheSidebarAndShowAllOpensThePage() {
        shell(mutableListOf(assistantJson("Stand-up", pinned = true), assistantJson("War room")))

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Stand-up, Idle").fetchSemanticsNodes().isNotEmpty() }
        assertEquals(0, rule.onAllNodesWithContentDescription("War room, Idle").fetchSemanticsNodes().size)

        rule.onNodeWithContentDescription("Show all assistants").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Open War room").fetchSemanticsNodes().isNotEmpty() }
        assertTrue(present("Daily summary") >= 1)
    }

    @Test
    fun withTheExperimentOffTheSidebarHasNoAssistants() {
        shell(mutableListOf(assistantJson("Stand-up", pinned = true)), experiments = EnabledExperiments.replace("\"assistants\":true", "\"assistants\":false"))

        rule.waitForIdle()

        assertEquals(0, present("Assistants"))
    }

    @Test
    fun creatingAnAssistantFromTheFormListsItOnThePage() {
        shell(mutableListOf(), Route.NewAssistant)

        rule.onNodeWithContentDescription("Name").performTextInput("Stand-up")
        rule.onNodeWithText("Create assistant").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Open Stand-up").fetchSemanticsNodes().isNotEmpty() }
        assertEquals("""{"name":"Stand-up","instructions":""}""", requests.last { it.first == "POST /api/v1/assistants" }.second)
    }

    @Test
    fun theRowMenuPinsAnAssistantIntoTheSidebar() {
        shell(mutableListOf(assistantJson("Stand-up")), Route.Assistants)

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("More for Stand-up").fetchSemanticsNodes().isNotEmpty() }
        assertEquals(0, rule.onAllNodesWithContentDescription("Stand-up, Idle").fetchSemanticsNodes().size)

        rule.onNodeWithContentDescription("More for Stand-up").performClick()
        rule.onNodeWithText("Pin").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Stand-up, Idle").fetchSemanticsNodes().isNotEmpty() }
        assertEquals("""{"pinned":true}""", requests.last { it.first.startsWith("PATCH") }.second)
    }
}
