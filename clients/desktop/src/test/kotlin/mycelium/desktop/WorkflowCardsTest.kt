package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.unit.dp
import mycelium.api.Workflow
import mycelium.api.WorkflowControlRequest
import mycelium.api.WorkflowKinds
import mycelium.api.WorkflowPlan
import mycelium.api.WorkflowRunStep
import mycelium.api.WorkflowStep
import mycelium.state.ThreadItem
import mycelium.state.WorkflowAgent
import mycelium.state.WorkflowProgress
import mycelium.ui.ConversationWorkflows
import mycelium.ui.LocalConversationWorkflows
import mycelium.ui.MyceliumTheme
import mycelium.ui.ThreadView
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.time.Clock
import kotlin.time.Duration.Companion.minutes

private val releasePlan = WorkflowPlan(
    maxRounds = 1,
    steps = listOf(
        WorkflowStep("build", "Build", agent = "codex"),
        WorkflowStep("signoff", "Sam’s sign-off", gate = true, dependsOn = listOf("build")),
    ),
)

private val releaseCheck = Workflow(
    id = "w1",
    title = "Release check",
    status = "waiting",
    plan = releasePlan,
    steps = listOf(
        WorkflowRunStep("build", "done", startedAt = "2025-03-04T09:00:00Z", endedAt = "2025-03-04T09:06:00Z"),
        WorkflowRunStep("signoff", "gate"),
    ),
    round = 1,
    conversationId = "c1",
    createdAt = "2025-03-04T09:00:00Z",
    updatedAt = "2025-03-04T09:06:00Z",
)

class WorkflowCardsTest {
    @get:Rule
    val rule = createComposeRule()

    private val controls = java.util.Collections.synchronizedList(mutableListOf<Pair<String, WorkflowControlRequest>>())
    private val opened = java.util.Collections.synchronizedList(mutableListOf<String>())
    private val stopped = java.util.Collections.synchronizedList(mutableListOf<String>())

    private fun show(items: List<ThreadItem>, workflows: List<Workflow>) {
        rule.setContent {
            MyceliumTheme(true) {
                CompositionLocalProvider(LocalConversationWorkflows provides ConversationWorkflows(workflows, { opened += it }, { id, request -> controls += id to request })) {
                    Box(Modifier.width(900.dp).height(700.dp)) {
                        ThreadView(items, onAnswer = { _, _ -> }, onStopWorkflow = { stopped += "stop" }, dismissedPins = setOf("workflow:run"))
                    }
                }
            }
        }
    }

    @Test
    fun aMyceliumWorkflowStartedFromTheConversationShowsItsStepsAndStatus() {
        show(emptyList(), listOf(releaseCheck))

        rule.onNodeWithText("Release check").assertIsDisplayed()
        rule.onNodeWithText("Waiting for you").assertIsDisplayed()
        rule.onNodeWithText("1 of 2").assertIsDisplayed()
        rule.onNodeWithText("Build").assertIsDisplayed()
        rule.onNodeWithText("Sam’s sign-off").assertIsDisplayed()
        rule.onNodeWithText("Codex, 6m").assertIsDisplayed()
    }

    @Test
    fun theTitleOpensTheWorkflowPage() {
        show(emptyList(), listOf(releaseCheck))

        rule.onNodeWithContentDescription("Open Release check").performClick()

        assertEquals(listOf("w1"), opened.toList())
    }

    @Test
    fun decideOnAGateRowOpensTheApproveDialogAndSendsTheDecision() {
        show(emptyList(), listOf(releaseCheck))

        rule.onNodeWithContentDescription("Decide Sam’s sign-off").performClick()
        rule.onNodeWithText("Approve").performClick()
        rule.onNodeWithText("Approve Sam’s sign-off?").assertIsDisplayed()
        rule.onNodeWithContentDescription("Approve and continue").performClick()

        assertEquals("w1", controls.single().first)
        assertEquals("approve", controls.single().second.action)
        assertEquals("signoff", controls.single().second.step)
    }

    @Test
    fun stoppingTheCardAsksFirst() {
        show(emptyList(), listOf(releaseCheck))

        rule.onNodeWithContentDescription("Stop Release check").performClick()
        rule.onNodeWithText("Stop Release check?").assertIsDisplayed()
        rule.onNodeWithText("Cancel").performClick()
        rule.waitForIdle()
        assertEquals(0, controls.size)

        rule.onNodeWithContentDescription("Stop Release check").performClick()
        rule.onNodeWithText("Stop workflow").performClick()

        assertEquals("stop", controls.single().second.action)
    }

    @Test
    fun anEndedWorkflowHasNoStopButton() {
        show(emptyList(), listOf(releaseCheck.copy(status = "done")))

        rule.onNodeWithText("Finished").assertIsDisplayed()
        assertEquals(0, rule.onAllNodesWithText("Stop").fetchSemanticsNodes().size)
    }

    @Test
    fun anAgentRunCardLinksToItsPageAndStopsOnlyAfterConfirmation() {
        val run = Workflow(id = "w9", title = "Build the usage items", status = "running", plan = WorkflowPlan(), steps = emptyList(), round = 1, kind = WorkflowKinds.AgentRun, runId = "run", conversationId = "c1")
        val agents = listOf(WorkflowAgent("a", "Usage card arrow", "Implement", finished = true), WorkflowAgent("b", "Activity all time", "Implement", finished = false))
        rule.setContent {
            MyceliumTheme(true) {
                CompositionLocalProvider(LocalConversationWorkflows provides ConversationWorkflows(listOf(run), { opened += it }, { _, _ -> })) {
                    Box(Modifier.width(900.dp).height(700.dp)) {
                        ThreadView(listOf(ThreadItem.Workflow("w", WorkflowProgress("run", agents, lastLineAt = Clock.System.now() - 1.minutes))), onAnswer = { _, _ -> }, onStopWorkflow = { stopped += "stop" }, dismissedPins = setOf("workflow:run"))
                    }
                }
            }
        }

        rule.onNodeWithContentDescription("Open Build the usage items").performClick()
        assertEquals(listOf("w9"), opened.toList())

        rule.onNodeWithContentDescription("Stop the workflow").performClick()
        assertEquals(0, stopped.size)

        rule.onNodeWithText("Stop workflow").performClick()
        assertEquals(listOf("stop"), stopped.toList())
    }
}
