package mycelium.desktop

import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextInput
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import mycelium.api.MyceliumApi
import mycelium.api.MyceliumJson
import mycelium.api.Workflow
import mycelium.api.WorkflowActor
import mycelium.api.WorkflowDecision
import mycelium.api.WorkflowEndReason
import mycelium.api.WorkflowKinds
import mycelium.api.WorkflowPlan
import mycelium.api.WorkflowRound
import mycelium.api.WorkflowRunStep
import mycelium.api.WorkflowStep
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.WorkflowScreen
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

private val plan = WorkflowPlan(
    title = "Restore flow hardening",
    maxRounds = 3,
    stopWhen = "RESTORE-OK",
    steps = listOf(
        WorkflowStep("research", "Research", agent = "codex", model = "gpt-5.5"),
        WorkflowStep("implement", "Implement", agent = "claude", dependsOn = listOf("research")),
        WorkflowStep("verify", "Verify", agent = "codex", dependsOn = listOf("implement")),
        WorkflowStep("review", "Review", gate = true, prompt = "Check the restore on a real snapshot before it ships.", dependsOn = listOf("verify")),
    ),
)

private fun ran(key: String, status: String, start: String, end: String?, summary: String, conversation: String? = null, tokens: Long? = null) =
    WorkflowRunStep(key = key, status = status, startedAt = "2025-03-04T$start:00Z", endedAt = end?.let { "2025-03-04T$it:00Z" }, summary = summary, conversationId = conversation, tokens = tokens)

private val waitingAtReview = Workflow(
    id = "w1",
    title = "Restore flow hardening",
    status = "waiting",
    plan = plan,
    round = 2,
    createdAt = "2025-03-04T09:12:00Z",
    updatedAt = "2025-03-04T10:30:00Z",
    startedBy = WorkflowActor("assistant", "Release captain"),
    stopRule = "Verify’s result contains RESTORE-OK",
    steps = listOf(
        ran("research", "done", "09:58", "10:04", "Streaming restore is possible.", "c1", 96_000),
        ran("implement", "done", "10:04", "10:21", "Raises the timeout to 5 minutes.", "c2", 410_000),
        ran("verify", "done", "10:21", "10:30", "All 31 restore tests pass.", "c3", 120_000),
        ran("review", "gate", "10:30", null, ""),
    ),
    rounds = listOf(
        WorkflowRound(
            round = 1,
            startedAt = "2025-03-04T09:12:00Z",
            endedAt = "2025-03-04T09:58:00Z",
            steps = listOf(
                ran("research", "done", "09:12", "09:19", "Reads the archive into memory."),
                ran("implement", "done", "09:19", "09:33", "Streams the restore through restic dump."),
                ran("verify", "done", "09:33", "09:41", "Timed out after 30 seconds."),
                ran("review", "sent_back", "09:41", "09:58", ""),
            ),
        ),
        WorkflowRound(round = 2, startedAt = "2025-03-04T09:58:00Z"),
    ),
    decisions = listOf(WorkflowDecision(1, "review", "send_back", WorkflowActor("person", "Sam"), "2025-03-04T09:58:00Z", "The 4 GB snapshot timed out.")),
)

class WorkflowPageTest {
    @get:Rule val rule = createComposeRule()
    @get:Rule val scopes = TestScopes()

    private val posts = java.util.Collections.synchronizedList(mutableListOf<Pair<String, String>>())
    private val opened = java.util.Collections.synchronizedList(mutableListOf<String>())

    private fun show(workflow: Workflow, rerun: Workflow? = null) {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val body = MyceliumJson.encodeToString(Workflow.serializer(), workflow)
        val engine = MockEngine { request ->
            if (request.method == HttpMethod.Post) {
                posts += request.url.encodedPath to String(request.body.toByteArray())
                respond(rerun?.takeIf { request.url.encodedPath.endsWith("/rerun") }?.let { MyceliumJson.encodeToString(Workflow.serializer(), it) } ?: body, HttpStatusCode.OK, json)
            } else {
                respond(body, HttpStatusCode.OK, json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", "test-token", engine), scopes.create())

        rule.setContent {
            MyceliumTheme(true) {
                val state by store.state.collectAsState()

                WorkflowScreen(store, state, workflow.id, onBack = {}, onOpenConversation = {}, onOpenWorkflow = { opened += it })
            }
        }
        rule.waitUntil(5000) { rule.onAllNodesWithText(workflow.title).fetchSemanticsNodes().isNotEmpty() }
    }

    private fun controlBodies() = posts.filter { it.first.endsWith("/control") }.map { Json.parseToJsonElement(it.second).jsonObject }

    @Test
    fun everyStepIsALaneInPlanOrderWithWhatItUses() {
        show(waitingAtReview)

        listOf("Research", "Implement", "Verify", "Review").forEach { rule.onNodeWithText(it).performScrollTo().assertIsDisplayed() }
        rule.onNodeWithText("Uses Research").performScrollTo().assertIsDisplayed()
        rule.onNodeWithText("Codex · gpt-5.5").performScrollTo().assertIsDisplayed()
        rule.onNodeWithText("Gate: waits for a decision").performScrollTo().assertIsDisplayed()
        rule.onNodeWithText("Raises the timeout to 5 minutes.").performScrollTo().assertIsDisplayed()
    }

    @Test
    fun theToolbarStatesTheRoundLimitStopRuleAndWhoStartedIt() {
        show(waitingAtReview)

        rule.onNodeWithText("Up to 3 rounds. Ends early when Verify’s result contains RESTORE-OK. Started 4 Mar 09:12 by Release captain.").assertIsDisplayed()
        rule.onNodeWithText("Round 1").assertIsDisplayed()
        rule.onNodeWithText("Round 2").assertIsDisplayed()
    }

    @Test
    fun theRoundNoteSaysWhoSentTheEarlierRoundBackAndWhy() {
        show(waitingAtReview)

        rule.onNodeWithText("Round 2 started at 4 Mar 09:58. Sam sent round 1 back at Review: “The 4 GB snapshot timed out.” Every step ran again with round 1’s results.").assertIsDisplayed()
    }

    @Test
    fun anEarlierRoundIsCollapsedInItsLaneAndExpandsToItsResult() {
        show(waitingAtReview)

        rule.onNodeWithText("Round 1: Finished, 14m").assertIsDisplayed()
        assertEquals(0, rule.onAllNodesWithText("Streams the restore through restic dump.").fetchSemanticsNodes().size)

        rule.onNodeWithText("Round 1: Finished, 14m").performClick()

        rule.onNodeWithText("Streams the restore through restic dump.").assertIsDisplayed()
    }

    @Test
    fun theGateLaneShowsWhenItStartedWaitingItsPromptAndTheEarlierDecision() {
        show(waitingAtReview)

        rule.onNodeWithText("Check the restore on a real snapshot before it ships.").performScrollTo().assertIsDisplayed()
        rule.onNodeWithText("4 Mar 10:30").performScrollTo().assertIsDisplayed()
        rule.onNodeWithText("Round 1: Sent back by Sam at 4 Mar 09:58").performScrollTo().assertIsDisplayed()
    }

    @Test
    fun sendingBackWithoutAReasonShowsTheErrorAndSendsNothing() {
        show(waitingAtReview)

        rule.onNodeWithContentDescription("Send Review back").performScrollTo().performClick()
        rule.onNodeWithText("Send Review back?").assertIsDisplayed()
        rule.onNodeWithText("The agents see this word for word.").assertIsDisplayed()
        rule.onNodeWithContentDescription("Send back and start round 3").performClick()

        rule.onNodeWithText("Enter what needs to change, so the next round can fix it").assertIsDisplayed()
        assertEquals(0, controlBodies().size)
    }

    @Test
    fun sendingBackWithAReasonPostsTheReasonForTheGateStep() {
        show(waitingAtReview)

        rule.onNodeWithContentDescription("Send Review back").performScrollTo().performClick()
        rule.onNodeWithContentDescription("What needs to change").performTextInput("Stream the restore")
        rule.onNodeWithContentDescription("Send back and start round 3").performClick()
        rule.waitUntil(5000) { controlBodies().isNotEmpty() }

        val request = controlBodies().single()
        assertEquals("send_back", request["action"]!!.toString().trim('"'))
        assertEquals("review", request["step"]!!.toString().trim('"'))
        assertEquals("Stream the restore", request["reason"]!!.toString().trim('"'))
    }

    @Test
    fun theLastRoundSaysSendingBackEndsTheWorkflow() {
        show(waitingAtReview.copy(round = 3))

        rule.onNodeWithContentDescription("Send Review back").performScrollTo().performClick()

        rule.onNodeWithText("Round 3 of 3 is the last round, so sending Review back ends the workflow. Your reason is kept with the decision.").assertIsDisplayed()
    }

    @Test
    fun approvingWithoutANoteIsAllowed() {
        show(waitingAtReview)

        rule.onNodeWithContentDescription("Approve Review").performScrollTo().performClick()
        rule.onNodeWithText("Approve Review?").assertIsDisplayed()
        rule.onNodeWithContentDescription("Approve and continue").performClick()
        rule.waitUntil(5000) { controlBodies().isNotEmpty() }

        val request = controlBodies().single()
        assertEquals("approve", request["action"]!!.toString().trim('"'))
        assertEquals("review", request["step"]!!.toString().trim('"'))
        assertNull(request["reason"])
    }

    @Test
    fun anApprovalNoteIsSentWithTheDecision() {
        show(waitingAtReview)

        rule.onNodeWithContentDescription("Approve Review").performScrollTo().performClick()
        rule.onNodeWithContentDescription("Note (optional)").performTextInput("Looks good")
        rule.onNodeWithContentDescription("Approve and continue").performClick()
        rule.waitUntil(5000) { controlBodies().isNotEmpty() }

        assertEquals("Looks good", controlBodies().single()["reason"]!!.toString().trim('"'))
    }

    @Test
    fun cancellingTheStopConfirmationStopsNothing() {
        show(waitingAtReview)

        rule.onNodeWithText("Stop").performClick()
        rule.onNodeWithText("Stop Restore flow hardening?").assertIsDisplayed()
        rule.onNodeWithText("Steps that are running stop where they are and the rest do not run. Their conversations stay, and you can re-run the workflow later.").assertIsDisplayed()
        rule.onNodeWithText("Cancel").performClick()
        rule.waitForIdle()

        assertEquals(0, controlBodies().size)
        assertEquals(0, rule.onAllNodesWithText("Stop workflow").fetchSemanticsNodes().size)
    }

    @Test
    fun confirmingTheStopSendsStop() {
        show(waitingAtReview)

        rule.onNodeWithText("Stop").performClick()
        rule.onNodeWithText("Stop workflow").performClick()
        rule.waitUntil(5000) { controlBodies().isNotEmpty() }

        assertEquals("stop", controlBodies().single()["action"]!!.toString().trim('"'))
    }

    @Test
    fun aFailedWorkflowNamesTheStepAndTheErrorAndLaterStepsDidNotRun() {
        show(
            waitingAtReview.copy(
                status = "failed",
                round = 1,
                rounds = emptyList(),
                decisions = emptyList(),
                endedAt = "2025-03-04T10:41:00Z",
                endReason = WorkflowEndReason("failed", step = "verify", error = "pnpm: command not found", round = 1),
                steps = listOf(
                    ran("research", "done", "09:58", "10:04", "Done."),
                    ran("implement", "done", "10:04", "10:21", "Done."),
                    ran("verify", "failed", "10:21", "10:41", "", "c3").copy(error = "pnpm: command not found"),
                    WorkflowRunStep("review", "not_run"),
                ),
            ),
        )

        rule.onNodeWithText("Verify failed in round 1: pnpm: command not found. Open Verify’s conversation to see the error, then re-run.").assertIsDisplayed()
        rule.onNodeWithText("Did not run because Verify failed.").assertIsDisplayed()
        rule.onNodeWithText("Re-run").assertIsDisplayed()
        assertEquals(0, rule.onAllNodesWithText("Stop").fetchSemanticsNodes().size)
        assertEquals(0, rule.onAllNodesWithText("Pause").fetchSemanticsNodes().size)
    }

    @Test
    fun rerunningOpensTheNewWorkflowPage() {
        val failed = waitingAtReview.copy(status = "failed", round = 1, rounds = emptyList(), decisions = emptyList(), endReason = WorkflowEndReason("failed", step = "verify", error = "boom", round = 1))
        show(failed, rerun = failed.copy(id = "w2", status = "running", rerunOf = "w1"))

        rule.onNodeWithText("Re-run").performClick()
        rule.waitUntil(5000) { opened.isNotEmpty() }

        assertEquals(listOf("w2"), opened.toList())
        assertTrue(posts.any { it.first.endsWith("/workflows/w1/rerun") })
    }

    @Test
    fun aStoppedWorkflowSaysWhoStoppedItWhenAndWhatWasRunning() {
        show(
            waitingAtReview.copy(
                status = "stopped",
                endedAt = "2025-03-04T10:12:00Z",
                endReason = WorkflowEndReason("stopped", by = WorkflowActor("person", "Sam"), round = 2),
                decisions = emptyList(),
                steps = listOf(
                    ran("research", "done", "09:58", "10:04", "Done."),
                    ran("implement", "stopped", "10:04", "10:12", "Stopped while editing restore.ts.", "c2"),
                    WorkflowRunStep("verify", "not_run"),
                    WorkflowRunStep("review", "not_run"),
                ),
            ),
        )

        rule.onNodeWithText("Stopped by Sam at 4 Mar 10:12 during round 2. Implement was running and stopped where it was. Verify and Review did not run.").assertIsDisplayed()
        rule.onNodeWithText("Re-run").assertIsDisplayed()
    }

    @Test
    fun anAgentRunShowsPhasesAsLanesWithoutRoundsGatesOrRerun() {
        val agents = listOf(
            WorkflowRunStep(key = "a", status = "done", label = "Usage card arrow", phase = "Implement", tokens = 410_000, turns = 14, elapsedMs = 540_000, summary = "The arrow opens Usage history."),
            WorkflowRunStep(key = "b", status = "running", label = "Activity all time", phase = "Implement"),
        )
        show(
            Workflow(
                id = "w9",
                title = "Build the usage items",
                status = "running",
                plan = WorkflowPlan(),
                steps = agents,
                round = 1,
                kind = WorkflowKinds.AgentRun,
                createdAt = "2025-03-04T08:40:00Z",
                updatedAt = "2099-01-01T00:00:00Z",
                phases = listOf("Implement", "Verify"),
            ),
        )

        rule.onNodeWithText("Running, 1 of 2 finished").assertIsDisplayed()
        rule.onNodeWithText("Usage card arrow").assertIsDisplayed()
        rule.onNodeWithText("The arrow opens Usage history.").assertIsDisplayed()
        rule.onNodeWithText("Waiting for Implement").assertIsDisplayed()
        rule.onNodeWithText("Stop").assertIsDisplayed()
        assertEquals(0, rule.onAllNodesWithText("Re-run").fetchSemanticsNodes().size)
        assertEquals(0, rule.onAllNodesWithText("Pause").fetchSemanticsNodes().size)
        assertEquals(0, rule.onAllNodesWithText("Round 1").fetchSemanticsNodes().size)
    }
}
