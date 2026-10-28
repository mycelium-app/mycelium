package mycelium

import mycelium.api.Workflow
import mycelium.api.WorkflowEndReason
import mycelium.api.WorkflowKinds
import mycelium.api.WorkflowPlan
import mycelium.api.WorkflowRunStep
import mycelium.api.WorkflowStep
import mycelium.state.WorkflowFilters
import mycelium.state.failedBecause
import mycelium.state.runMinutes
import mycelium.state.runTokens
import mycelium.state.stalled
import mycelium.state.visibleWorkflows
import mycelium.state.waitingFor
import mycelium.state.workflowWhere
import mycelium.ui.stepStatusLook
import mycelium.ui.workflowStatusLook
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue
import kotlin.time.Instant

private val reviewPlan = WorkflowPlan(
    title = "Harden the restore flow",
    maxRounds = 3,
    steps = listOf(
        WorkflowStep("implement", "Implement"),
        WorkflowStep("verify", "Verify", dependsOn = listOf("implement")),
        WorkflowStep("review", "Review", gate = true, dependsOn = listOf("verify")),
    ),
)

private fun workflow(
    id: String,
    title: String,
    status: String,
    createdAt: String = "2025-03-04T09:00:00Z",
    projectId: String? = null,
    round: Int = 1,
    steps: List<WorkflowRunStep> = emptyList(),
    plan: WorkflowPlan = reviewPlan,
    kind: String = WorkflowKinds.Mycelium,
    endReason: WorkflowEndReason? = null,
    updatedAt: String = createdAt,
) = Workflow(id = id, title = title, status = status, plan = plan, steps = steps, round = round, kind = kind, createdAt = createdAt, updatedAt = updatedAt, projectId = projectId, endReason = endReason)

private fun step(key: String, status: String) = WorkflowRunStep(key = key, status = status)

class WorkflowListTest {
    private val all = listOf(
        workflow("a", "Restore flow hardening", "waiting", "2025-03-04T09:12:00Z", "harbour"),
        workflow("b", "Usage items", "running", "2025-03-04T08:40:00Z", "ledger"),
        workflow("c", "Renovate sweep", "paused", "2025-03-04T07:55:00Z", "atlas"),
        workflow("d", "Nightly dependency audit", "done", "2025-03-03T02:00:00Z", "atlas"),
        workflow("e", "Uploads survive a 503", "failed", "2025-03-03T16:05:00Z", "ledger"),
        workflow("f", "Follow-ups", "stopped", "2025-03-02T10:00:00Z", null),
    )

    private fun ids(filter: String = WorkflowFilters.All, query: String = "", project: String? = null) = visibleWorkflows(all, filter, query, project).map { it.id }

    @Test
    fun theListIsNewestFirst() {
        assertEquals(listOf("a", "b", "c", "e", "d", "f"), ids())
    }

    @Test
    fun activeShowsRunningWaitingAndPausedWorkflows() {
        assertEquals(listOf("a", "b", "c"), ids(WorkflowFilters.Active))
    }

    @Test
    fun waitingForYouShowsOnlyWorkflowsWithAnOpenGate() {
        assertEquals(listOf("a"), ids(WorkflowFilters.Waiting))
    }

    @Test
    fun endedShowsFinishedFailedAndStoppedWorkflows() {
        assertEquals(listOf("e", "d", "f"), ids(WorkflowFilters.Ended))
    }

    @Test
    fun theProjectFilterKeepsOnlyThatProject() {
        assertEquals(listOf("b", "e"), ids(project = "ledger"))
    }

    @Test
    fun searchMatchesTheTitleIgnoringCase() {
        assertEquals(listOf("e"), ids(query = " UPLOADS "))
    }

    @Test
    fun filtersCombine() {
        assertEquals(listOf("e"), ids(WorkflowFilters.Ended, "503", "ledger"))
        assertEquals(emptyList(), ids(WorkflowFilters.Active, "503"))
    }

    @Test
    fun aWaitingWorkflowSaysWhichStepAndRoundItIsAt() {
        val review = workflow("a", "r", "waiting", round = 2, steps = listOf(step("implement", "done"), step("verify", "done"), step("review", "gate")))

        assertEquals("Review, round 2 of 3", workflowWhere(review))
    }

    @Test
    fun anEndedWorkflowSaysHowItEnded() {
        val steps = listOf(step("implement", "done"), step("verify", "done"), step("review", "approved"))

        assertEquals("3 steps, 1 round", workflowWhere(workflow("a", "r", "done", steps = steps)))
        assertEquals("Verify failed in round 1", workflowWhere(workflow("a", "r", "failed", endReason = WorkflowEndReason("failed", step = "verify", round = 1))))
        assertEquals("Stopped before any step ran", workflowWhere(workflow("a", "r", "stopped", steps = listOf(step("implement", "not_run")))))
        assertEquals("Stopped in round 2", workflowWhere(workflow("a", "r", "stopped", round = 2, steps = listOf(step("implement", "stopped")))))
    }

    @Test
    fun anAgentRunSaysWhichPhaseAndHowManyAgentsHaveFinished() {
        val agents = listOf("a", "b", "c", "d", "e").mapIndexed { index, key ->
            WorkflowRunStep(key = key, status = if (index < 2) "done" else "running", label = key, phase = "Implement")
        }
        val run = workflow("a", "r", "running", kind = WorkflowKinds.AgentRun, steps = agents)

        assertEquals("Implement, 2 of 5 agents", workflowWhere(run))
    }

    @Test
    fun anAgentRunWithNoUpdateForFifteenMinutesIsStalled() {
        val run = workflow("a", "r", "running", kind = WorkflowKinds.AgentRun, updatedAt = "2025-03-04T09:00:00Z")

        assertTrue(run.stalled(Instant.parse("2025-03-04T09:15:00Z")))
        assertFalse(run.stalled(Instant.parse("2025-03-04T09:14:00Z")))
        assertFalse(workflow("a", "r", "running", updatedAt = "2025-03-04T09:00:00Z").stalled(Instant.parse("2025-03-04T10:00:00Z")))
    }

    @Test
    fun finishedIsTheOnlyWordForADoneWorkflowAndItsSteps() {
        assertEquals("Finished", workflowStatusLook("done").label)
        assertEquals("Finished", stepStatusLook("done").label)
        assertEquals("Waiting for you", workflowStatusLook("waiting").label)
        assertEquals("Stalled", workflowStatusLook("running", stalled = true).label)
        assertEquals("Did not run", stepStatusLook("not_run").label)
        assertEquals("Sent back", stepStatusLook("sent_back").label)
        assertEquals("Waiting for Verify", stepStatusLook("waiting", "Verify").label)
    }

    @Test
    fun aWaitingStepNamesTheFirstDependencyThatIsNotFinished() {
        val steps = listOf(step("implement", "running"), step("verify", "waiting"))
        val run = workflow("a", "r", "running", steps = steps)

        assertEquals("Implement", run.waitingFor("verify", steps))
        assertNull(run.waitingFor("implement", steps))
    }

    @Test
    fun laterStepsOfAFailedWorkflowSayWhichStepFailed() {
        val steps = listOf(step("implement", "done"), step("verify", "failed"), step("review", "not_run"))
        val run = workflow("a", "r", "failed", steps = steps, endReason = WorkflowEndReason("failed", step = "verify"))

        assertEquals("Verify", run.failedBecause("review", steps))
        assertNull(run.failedBecause("verify", steps))
        assertNull(run.failedBecause("implement", steps))
    }

    @Test
    fun durationsAndTokensAreShortened() {
        assertEquals("6m", runMinutes("2025-03-04T09:58:00Z", "2025-03-04T10:04:30Z"))
        assertEquals("under 1m", runMinutes("2025-03-04T09:58:00Z", "2025-03-04T09:58:20Z"))
        assertEquals("1h 05m", runMinutes("2025-03-04T09:00:00Z", "2025-03-04T10:05:00Z"))
        assertNull(runMinutes("2025-03-04T09:00:00Z", null))
        assertEquals("96k", runTokens(96_400))
        assertEquals("1.4M", runTokens(1_400_000))
        assertEquals("812", runTokens(812))
    }
}
