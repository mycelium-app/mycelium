package mycelium.desktop

import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isSelectable
import androidx.compose.ui.test.junit4.createComposeRule
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
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import mycelium.api.MyceliumApi
import mycelium.api.MyceliumJson
import mycelium.api.Workflow
import mycelium.api.WorkflowPlan
import mycelium.api.WorkflowRunStep
import mycelium.api.WorkflowStep
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.WorkflowsScreen
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private val reviewPlan = WorkflowPlan(maxRounds = 3, steps = listOf(WorkflowStep("build", "Build"), WorkflowStep("review", "Review", gate = true)))

private fun workflow(id: String, title: String, status: String, createdAt: String, steps: List<WorkflowRunStep> = emptyList(), round: Int = 1) =
    Workflow(id = id, title = title, status = status, plan = reviewPlan, steps = steps, round = round, createdAt = createdAt, updatedAt = createdAt)

class WorkflowListScreenTest {
    @get:Rule val rule = createComposeRule()
    @get:Rule val scopes = TestScopes()

    private val posts = java.util.Collections.synchronizedList(mutableListOf<Pair<String, String>>())
    private val opened = java.util.Collections.synchronizedList(mutableListOf<String>())

    private val fixtures = listOf(
        workflow("a", "Restore flow hardening", "waiting", "2025-03-04T09:12:00Z", listOf(WorkflowRunStep("build", "done"), WorkflowRunStep("review", "gate")), round = 2),
        workflow("b", "Nightly dependency audit", "done", "2025-03-03T02:00:00Z", listOf(WorkflowRunStep("build", "done"), WorkflowRunStep("review", "approved"))),
        workflow("c", "Usage items", "running", "2025-03-04T08:40:00Z", listOf(WorkflowRunStep("build", "running"), WorkflowRunStep("review", "waiting"))),
    )

    private fun show(workflows: List<Workflow> = fixtures) {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            if (request.method == HttpMethod.Post) {
                posts += request.url.encodedPath to String(request.body.toByteArray())
                respond(MyceliumJson.encodeToString(Workflow.serializer(), workflows.first()), HttpStatusCode.OK, json)
            } else {
                respond(MyceliumJson.encodeToString(ListSerializer(Workflow.serializer()), workflows), HttpStatusCode.OK, json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", "test-token", engine), scopes.create())

        rule.setContent {
            MyceliumTheme(true) {
                val state by store.state.collectAsState()

                WorkflowsScreen(store, state, onOpenConversation = {}, onOpenWorkflow = { opened += it })
            }
        }
    }

    private fun segment(name: String) = rule.onNode(hasText(name) and isSelectable())

    @Test
    fun rowsShowStatusWhereItIsAndWhenItStarted() {
        show()
        rule.waitUntil(5000) { rule.onAllNodesWithText("Restore flow hardening").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithText("Review, round 2 of 3").assertIsDisplayed()
        rule.onNodeWithText("2 steps, 1 round").assertIsDisplayed()
        rule.onNodeWithText("Build, round 1 of 3").assertIsDisplayed()
        rule.onNodeWithText("4 Mar 09:12").assertIsDisplayed()
        rule.onAllNodesWithText("Workflows page").fetchSemanticsNodes().let { assertEquals(3, it.size) }
    }

    @Test
    fun theWaitingForYouSegmentKeepsOnlyWorkflowsWithAnOpenGate() {
        show()
        rule.waitUntil(5000) { rule.onAllNodesWithText("Restore flow hardening").fetchSemanticsNodes().isNotEmpty() }

        segment("Waiting for you").performClick()

        rule.onNodeWithText("Restore flow hardening").assertIsDisplayed()
        assertEquals(0, rule.onAllNodesWithText("Usage items").fetchSemanticsNodes().size)
        assertEquals(0, rule.onAllNodesWithText("Nightly dependency audit").fetchSemanticsNodes().size)
    }

    @Test
    fun theEndedSegmentKeepsOnlyFinishedFailedAndStoppedWorkflows() {
        show()
        rule.waitUntil(5000) { rule.onAllNodesWithText("Restore flow hardening").fetchSemanticsNodes().isNotEmpty() }

        segment("Ended").performClick()

        rule.onNodeWithText("Nightly dependency audit").assertIsDisplayed()
        assertEquals(0, rule.onAllNodesWithText("Restore flow hardening").fetchSemanticsNodes().size)
    }

    @Test
    fun aSearchWithNoMatchSaysSo() {
        show()
        rule.waitUntil(5000) { rule.onAllNodesWithText("Restore flow hardening").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithContentDescription("Search workflows").performTextInput("zzz")

        rule.onNodeWithText("No workflow matches.").assertIsDisplayed()
    }

    @Test
    fun withNoWorkflowsTheEmptyStateInvitesYouToStartOne() {
        show(emptyList())

        rule.waitUntil(5000) { rule.onAllNodesWithText("No workflows yet").fetchSemanticsNodes().isNotEmpty() }
        assertEquals(2, rule.onAllNodesWithText("New workflow").fetchSemanticsNodes().size)
    }

    @Test
    fun choosingARowOpensItsPage() {
        show()
        rule.waitUntil(5000) { rule.onAllNodesWithText("Usage items").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithContentDescription("Open Usage items").performClick()

        assertEquals(listOf("c"), opened.toList())
    }

    @Test
    fun stoppingFromTheRowMenuAsksFirstAndThenSendsStop() {
        show()
        rule.waitUntil(5000) { rule.onAllNodesWithText("Usage items").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithContentDescription("More for Usage items").performClick()
        rule.onNodeWithText("Stop…").performClick()
        rule.onNodeWithText("Stop Usage items?").assertIsDisplayed()
        assertEquals(0, posts.size)

        rule.onNodeWithText("Stop workflow").performClick()
        rule.waitUntil(5000) { posts.isNotEmpty() }

        assertEquals("/api/v1/workflows/c/control", posts.single().first)
        assertEquals("\"stop\"", Json.parseToJsonElement(posts.single().second).jsonObject["action"].toString())
    }

    @Test
    fun anEndedRowOffersReRunInsteadOfStop() {
        show()
        rule.waitUntil(5000) { rule.onAllNodesWithText("Nightly dependency audit").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithContentDescription("More for Nightly dependency audit").performClick()

        rule.onNodeWithText("Re-run").assertIsDisplayed()
        assertEquals(0, rule.onAllNodesWithText("Stop…").fetchSemanticsNodes().size)
    }
}
