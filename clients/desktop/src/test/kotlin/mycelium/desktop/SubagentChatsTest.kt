package mycelium.desktop

import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
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

private const val Parent = """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/app","title":"Review the change","status":"done","acp_session_id":"s","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z"}"""
private const val Subagent = """{"id":"c2","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/app","title":"Check the build","status":"done","created_at":"2026-10-22T10:01:00Z","updated_at":"2026-10-22T10:02:00Z","parent_id":"c1","side_kind":"subagent"}"""

private object SubagentPlatform : Platform {
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

class SubagentChatsTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private fun shell() {
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val body = when {
                path.endsWith("/conversations") && request.url.parameters["archived"] != "true" -> "[$Parent,$Subagent]"
                path.endsWith("/conversations/c1") -> Parent
                path.endsWith("/conversations/c2") -> Subagent
                path.endsWith("/events") || path.endsWith("/queue") || path.endsWith("/nodes") || path.endsWith("/conversations") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json"))
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { Shell(store, SubagentPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Empty) } }
    }

    private fun openSubagentsOfTheParent() {
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Review the change, ", substring = true).fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Review the change, ", substring = true).performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Open subagents").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Open subagents").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Check the build, ", substring = true).fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun aSubagentStaysOutOfTheSidebarAndIsListedBesideItsConversation() {
        shell()

        openSubagentsOfTheParent()

        assertEquals(0, rule.onAllNodesWithContentDescription(", subagent", substring = true).fetchSemanticsNodes().size)
        assertEquals(1, rule.onAllNodesWithContentDescription("Check the build, ", substring = true).fetchSemanticsNodes().size)
    }

    @Test
    fun anOpenedSubagentHasNoComposerAndLinksBackToItsConversation() {
        shell()
        openSubagentsOfTheParent()

        rule.onNodeWithContentDescription("Check the build, ", substring = true).performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Open the conversation it belongs to").fetchSemanticsNodes().isNotEmpty() }
        assertEquals(0, rule.onAllNodes(hasSetTextAction()).fetchSemanticsNodes().size)

        rule.onNodeWithText("Open the conversation it belongs to").performClick()

        rule.waitUntil(5_000) { rule.onAllNodes(hasSetTextAction()).fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun aSubagentRowShowsHowLongAgoItWasLastActive() {
        shell()

        openSubagentsOfTheParent()

        val description = rule.onNodeWithContentDescription("Check the build, ", substring = true).fetchSemanticsNode().config[SemanticsProperties.ContentDescription].single()

        assertEquals(3, description.split(", ").size)
        assertTrue(description.split(", ").last().isNotBlank())
    }

    @Test
    fun insideASubagentThePanelStillListsItsParentsSubagentsAndLeadsBack() {
        shell()
        openSubagentsOfTheParent()

        rule.onNodeWithContentDescription("Check the build, ", substring = true).performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Open the conversation it belongs to").fetchSemanticsNodes().isNotEmpty() }

        if (rule.onAllNodesWithContentDescription("Open subagents").fetchSemanticsNodes().isNotEmpty()) {
            rule.onNodeWithContentDescription("Open subagents").performClick()
        }

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Back to Review the change").fetchSemanticsNodes().isNotEmpty() }
        assertEquals(1, rule.onAllNodesWithContentDescription("Check the build, ", substring = true).fetchSemanticsNodes().size)

        rule.onNodeWithContentDescription("Back to Review the change").performClick()

        rule.waitUntil(5_000) { rule.onAllNodes(hasSetTextAction()).fetchSemanticsNodes().isNotEmpty() }
    }
}
