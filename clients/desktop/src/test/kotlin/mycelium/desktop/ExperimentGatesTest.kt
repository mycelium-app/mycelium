package mycelium.desktop

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.performClick
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
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

private const val Chat = """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/app","title":"Backups","status":"done","acp_session_id":"s","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z"}"""
private const val Node = """[{"id":"n1","name":"willow","hostname":"willow","os":"linux","arch":"x86_64","version":"v0.1.10","address":"10.0.0.5","online":true,"draining":false,"sessions":["c1"],"agents":[{"agent":"claude","available":true}],"logins":[]}]"""
private const val AllOff = """{"voice":false}"""

private object GatesPlatform : Platform {
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

class ExperimentGatesTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private fun shell(experiments: String?) {
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val body = when {
                path.endsWith("/experiments") -> experiments ?: "[]"
                path.endsWith("/conversations/c1") -> Chat
                path.endsWith("/conversations") && request.url.parameters["archived"] != "true" -> "[$Chat]"
                path.endsWith("/nodes") -> Node
                path.endsWith("/events") || path.endsWith("/queue") || path.endsWith("/conversations") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json"))
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { Shell(store, GatesPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Chat("c1")) } }
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Open git").fetchSemanticsNodes().isNotEmpty() }
    }

    private fun present(description: String) = rule.onAllNodesWithContentDescription(description).fetchSemanticsNodes().size

    @Test
    fun withEveryExperimentOffTheSubagentsPanelIsOffered() {
        shell(AllOff)

        assertEquals(1, present("Open subagents"))
        assertEquals(1, present("Open git"))
    }

    @Test
    fun withEveryExperimentOffTheNodeMenuOffersMove() {
        shell(AllOff)

        rule.onNodeWithContentDescription("Running on willow, open the node menu").performClick()
        rule.waitForIdle()

        assertEquals(1, rule.onAllNodesWithText("Move").fetchSemanticsNodes().size)
    }

    @Test
    fun withEveryExperimentOffTheSidebarHidesTasksWorkflowsAndHistory() {
        shell(AllOff)

        assertEquals(0, rule.onAllNodesWithText("Tasks").fetchSemanticsNodes().size)
        assertEquals(0, rule.onAllNodesWithText("Workflows").fetchSemanticsNodes().size)
        assertEquals(0, rule.onAllNodesWithText("History").fetchSemanticsNodes().size)
    }

    @Test
    fun turningOnTheTasksAndWorkflowsAndHistoryExperimentsShowsThemInTheSidebar() {
        shell("""{"tasks_and_workflows":true,"history":true}""")

        assertEquals(1, rule.onAllNodesWithText("Tasks").fetchSemanticsNodes().size)
        assertEquals(1, rule.onAllNodesWithText("Workflows").fetchSemanticsNodes().size)
        assertEquals(1, rule.onAllNodesWithText("History").fetchSemanticsNodes().size)
    }

    @Test
    fun afterAFailedExperimentsFetchTheRestOfTheShellStillLoads() {
        shell(null)

        assertTrue(rule.onAllNodesWithText("Artefacts").fetchSemanticsNodes().isNotEmpty())
    }
}
