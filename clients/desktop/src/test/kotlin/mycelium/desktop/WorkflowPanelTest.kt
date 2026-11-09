package mycelium.desktop

import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasContentDescription
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

private const val ParentConversation = """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/app","title":"Review the change","status":"review","acp_session_id":"s","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z"}"""
private const val StepConversation = """{"id":"c2","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/app","title":"Check the build","status":"review","created_at":"2026-10-22T10:01:00Z","updated_at":"2026-10-22T10:02:00Z","parent_id":"c1","side_kind":"subagent"}"""
private const val PinWorkflow = """{"id":"w1","conversation_id":"c1","title":"Pin the UI kit","status":"waiting","round":1,"created_at":"2026-10-22T10:02:00Z","updated_at":"2026-10-22T10:03:00Z","plan":{"steps":[{"key":"find","title":"Find the pins","agent":"claude","prompt":"Find them."},{"key":"signoff","title":"Install Chromium","gate":true,"prompt":"Install it?","depends_on":["find"]},{"key":"test","title":"Run the suite","agent":"claude","prompt":"Run it.","depends_on":["signoff"]}]},"steps":[{"key":"find","status":"done","conversation_id":"c2","started_at":"2026-10-22T10:02:00Z","ended_at":"2026-10-22T10:06:00Z"},{"key":"signoff","status":"gate"}]}"""

class WorkflowPanelTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val controls = java.util.concurrent.CopyOnWriteArrayList<String>()

    private fun shell(route: Route = Route.Empty) {
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            if (request.method == HttpMethod.Post && path.endsWith("/workflows/w1/control")) {
                controls += String(request.body.toByteArray())
            }

            val body = when {
                path.endsWith("/experiments") -> EnabledExperiments
                path.endsWith("/workflows") -> "[$PinWorkflow]"
                path.endsWith("/workflows/w1/control") -> PinWorkflow.replace("\"status\":\"gate\"", "\"status\":\"done\"")
                path.endsWith("/conversations") && request.url.parameters["archived"] != "true" -> "[$ParentConversation,$StepConversation]"
                path.endsWith("/conversations/c1") -> ParentConversation
                path.endsWith("/events") || path.endsWith("/queue") || path.endsWith("/nodes") || path.endsWith("/conversations") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json"))
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { Shell(store, PlatformStub, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = route) } }
    }

    private fun openTheConversation() {
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Review the change, ", substring = true).fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Review the change, ", substring = true).performClick()
    }

    private fun openTheHistory() {
        openTheConversation()
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Open subagents and workflows").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Open subagents and workflows").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Workflow Pin the UI kit, ", substring = true).fetchSemanticsNodes().isNotEmpty() }
    }

    private fun openTheWorkflow() {
        openTheHistory()
        rule.onNodeWithContentDescription("Workflow Pin the UI kit, ", substring = true).performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Approve Install Chromium").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun theSidebarHasNoWorkflowsLink() {
        shell()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Artefacts").fetchSemanticsNodes().isNotEmpty() }
        assertEquals(0, rule.onAllNodesWithText("Workflows").fetchSemanticsNodes().size)
    }

    @Test
    fun theSidePanelListsAWorkflowAndASubagentTogetherNewestFirst() {
        shell()
        openTheHistory()

        val workflowTop = rule.onNodeWithContentDescription("Workflow Pin the UI kit, ", substring = true).fetchSemanticsNode().boundsInRoot.top
        val subagentTop = rule.onNode(hasContentDescription("Check the build, ", substring = true) and hasAnyAncestor(hasContentDescription("Subagents and workflows"))).fetchSemanticsNode().boundsInRoot.top

        assertTrue(workflowTop < subagentTop)
        rule.onNodeWithText("Workflow · Waiting for you").assertIsDisplayed()
        rule.onNodeWithText("Subagent · ", substring = true).assertIsDisplayed()
        rule.onNodeWithText("Subagents and workflows").assertIsDisplayed()
        rule.captureScreen("2.5-side-panel-history")
    }

    @Test
    fun choosingAWorkflowOpensItsStepsAndItsGate() {
        shell()
        openTheWorkflow()

        rule.onNodeWithContentDescription("Step 1, Find the pins, Finished, open its conversation").assertIsDisplayed()
        rule.onNodeWithContentDescription("Gate, Install Chromium, waiting for you").assertIsDisplayed()
        rule.onNodeWithContentDescription("Decline Install Chromium").assertIsDisplayed()
        rule.onNodeWithContentDescription("Step 3, Run the suite, Waiting").assertIsDisplayed()
        rule.captureScreen("2.55-workflow-in-side-panel")

        rule.onNodeWithContentDescription("Back to subagents and workflows").performClick()

        rule.onNodeWithContentDescription("Workflow Pin the UI kit, ", substring = true).assertIsDisplayed()
    }

    @Test
    fun approvingAGateFromThePanelSendsTheApproveRequestTheGateCardSends() {
        shell()
        openTheWorkflow()

        rule.onNodeWithContentDescription("Approve Install Chromium").performClick()
        rule.waitUntil(5_000) { controls.isNotEmpty() }

        assertEquals("""{"action":"approve","step":"signoff"}""", controls.single().replace(" ", ""))
    }

    @Test
    fun theWorkflowNameOnACardOpensItInTheSidePanel() {
        shell()
        openTheConversation()
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Open Pin the UI kit").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithContentDescription("Open Pin the UI kit").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Approve Install Chromium").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Back to subagents and workflows").assertIsDisplayed()
    }
}
