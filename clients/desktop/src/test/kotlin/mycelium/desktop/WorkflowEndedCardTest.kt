package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.width
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.unit.dp
import mycelium.state.ThreadItem
import mycelium.state.WorkflowAgent
import mycelium.state.WorkflowEnd
import mycelium.state.WorkflowProgress
import mycelium.ui.MyceliumTheme
import mycelium.ui.ThreadView
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.time.Clock
import kotlin.time.Duration.Companion.minutes
import kotlin.time.Instant

class WorkflowEndedCardTest {
    @get:Rule
    val rule = createComposeRule()

    private fun show(progress: WorkflowProgress) {
        rule.setContent {
            MyceliumTheme(true) {
                Box(Modifier.width(900.dp).height(700.dp)) {
                    ThreadView(listOf(ThreadItem.Workflow("w", progress)), onAnswer = { _, _ -> }, onStopWorkflow = {}, dismissedPins = setOf("workflow:run"))
                }
            }
        }
    }

    @Test
    fun aWorkflowStoppedBeforeAnyAgentRanSaysSoWithoutAStopButtonOrASpinner() {
        show(WorkflowProgress("run", emptyList(), plannedPhases = listOf("Implement", "Verify", "Fix"), end = WorkflowEnd("stopped", Instant.parse("2026-10-27T00:42:10Z"))))

        rule.onNodeWithText("Stopped before any step ran").assertIsDisplayed()
        rule.onNodeWithText("before the first agent started.", substring = true).assertIsDisplayed()
        assertEquals(3, rule.onAllNodesWithText("Did not run").fetchSemanticsNodes().size)
        assertEquals(0, rule.onAllNodesWithContentDescription("Stop the workflow").fetchSemanticsNodes().size)
        assertEquals(0, rule.onAllNodesWithText("Waiting").fetchSemanticsNodes().size)
    }

    @Test
    fun aFailedWorkflowShowsTheReasonAndMarksPhasesThatNeverStarted() {
        val agents = listOf(WorkflowAgent("a", "Usage card arrow", "Implement", finished = true), WorkflowAgent("b", "Usage history page", "Implement", finished = true, failed = true))
        show(WorkflowProgress("run", agents, plannedPhases = listOf("Implement", "Verify"), end = WorkflowEnd("failed", reason = "The weekly limit was reached and the workflow ended.")))

        rule.onNodeWithText("Failed").assertIsDisplayed()
        rule.onNodeWithText("The weekly limit was reached and the workflow ended.").assertIsDisplayed()
        rule.onNodeWithText("Did not run").assertIsDisplayed()
        assertEquals(0, rule.onAllNodesWithContentDescription("Stop the workflow").fetchSemanticsNodes().size)
    }

    @Test
    fun aRunningWorkflowWithNoJournalLineForFifteenMinutesIsStalledAndCanBeStopped() {
        val agents = listOf(WorkflowAgent("a", "Activity all time", "Implement", finished = true), WorkflowAgent("b", "Sidebar row menus", "Implement", finished = false))
        show(WorkflowProgress("run", agents, lastLineAt = Clock.System.now() - 25.minutes))

        rule.onNodeWithText("No progress for 25 minutes").assertIsDisplayed()
        rule.onNodeWithText("last line 25m ago").assertIsDisplayed()
        rule.onNodeWithContentDescription("Stop the workflow").assertIsDisplayed()
    }
}
