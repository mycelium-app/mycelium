package mycelium.state

import mycelium.api.Conversation
import mycelium.api.Workflow
import mycelium.api.WorkflowDecision
import mycelium.api.WorkflowKinds
import mycelium.api.WorkflowRound
import mycelium.api.WorkflowStep
import mycelium.api.WorkflowRunStep
import mycelium.api.WorkflowStatuses
import kotlin.time.Duration
import kotlin.time.Instant

object WorkflowFilters {
    const val All = "All"
    const val Active = "Active"
    const val Waiting = "Waiting for you"
    const val Ended = "Ended"

    val Options = listOf(All, Active, Waiting, Ended)
}

object WorkflowStepStatuses {
    const val Waiting = "waiting"
    const val Starting = "starting"
    const val Running = "running"
    const val Gate = "gate"
    const val Approved = "approved"
    const val SentBack = "sent_back"
    const val Done = "done"
    const val Failed = "failed"
    const val Stopped = "stopped"
    const val NotRun = "not_run"
    const val Paused = "paused"
}

object WorkflowActions {
    const val Pause = "pause"
    const val Resume = "resume"
    const val Stop = "stop"
    const val Approve = "approve"
    const val SendBack = "send_back"
}

private val EndedStatuses = setOf(WorkflowStatuses.Done, WorkflowStatuses.Failed, WorkflowStatuses.Stopped)

fun Workflow.ended(): Boolean = status in EndedStatuses

fun Workflow.stalled(now: Instant): Boolean {
    val updated = instantOrNull(updatedAt) ?: return false

    return kind == WorkflowKinds.AgentRun && status == WorkflowStatuses.Running && now - updated >= WorkflowStalledAfter
}

fun Workflow.stepTitle(key: String?): String = plan.steps.firstOrNull { it.key == key }?.title ?: key.orEmpty()

fun visibleWorkflows(workflows: Collection<Workflow>, filter: String, query: String, projectId: String?): List<Workflow> {
    val needle = query.trim()

    return workflows
        .filter { workflow ->
            when (filter) {
                WorkflowFilters.Active -> !workflow.ended()
                WorkflowFilters.Waiting -> workflow.status == WorkflowStatuses.Waiting
                WorkflowFilters.Ended -> workflow.ended()
                else -> true
            }
        }
        .filter { projectId == null || it.projectId == projectId }
        .filter { needle.isEmpty() || it.title.contains(needle, ignoreCase = true) }
        .sortedWith(compareByDescending<Workflow> { it.createdAt }.thenBy { it.id })
}

private fun plural(count: Int, noun: String): String = "$count $noun${if (count == 1) "" else "s"}"

fun workflowWhere(workflow: Workflow): String = if (workflow.kind == WorkflowKinds.AgentRun) agentRunWhere(workflow) else myceliumWhere(workflow)

private fun myceliumWhere(workflow: Workflow): String {
    val rounds = "round ${workflow.round} of ${workflow.plan.maxRounds}"

    return when (workflow.status) {
        WorkflowStatuses.Done -> "${plural(workflow.plan.steps.size, "step")}, ${plural(workflow.round, "round")}"
        WorkflowStatuses.Failed -> workflow.endReason?.step?.let { "${workflow.stepTitle(it)} failed in round ${workflow.endReason?.round ?: workflow.round}" } ?: "Failed in round ${workflow.round}"
        WorkflowStatuses.Stopped -> if (workflow.steps.none { it.status != WorkflowStepStatuses.Waiting && it.status != WorkflowStepStatuses.NotRun } && workflow.round <= 1) "Stopped before any step ran" else "Stopped in round ${workflow.round}"
        else -> {
            val current = workflow.steps.firstOrNull { it.status in setOf(WorkflowStepStatuses.Gate, WorkflowStepStatuses.Running, WorkflowStepStatuses.Starting) }
                ?: workflow.steps.firstOrNull { it.status == WorkflowStepStatuses.Waiting }

            if (current == null) rounds.replaceFirstChar { it.uppercase() } else "${workflow.stepTitle(current.key)}, $rounds"
        }
    }
}

private fun agentRunWhere(workflow: Workflow): String {
    val agents = workflow.steps
    val finished = agents.count { it.status == WorkflowStepStatuses.Done }
    val unfinished = agents.firstOrNull { it.status != WorkflowStepStatuses.Done }
    val phase = (unfinished ?: agents.lastOrNull())?.phase ?: workflow.phases.firstOrNull()

    return when (workflow.status) {
        WorkflowStatuses.Done -> plural(agents.size, "agent")
        WorkflowStatuses.Failed -> unfinished?.label?.let { "$it failed" } ?: "Failed"
        WorkflowStatuses.Stopped -> "Stopped with $finished of ${plural(agents.size, "agent")} finished"
        else -> if (agents.isEmpty()) phase ?: "Starting" else "${phase ?: "Running"}, $finished of ${plural(agents.size, "agent")}"
    }
}

fun runMinutes(start: String?, end: String?): String? {
    val from = start?.let(::instantOrNull) ?: return null
    val to = end?.let(::instantOrNull) ?: return null
    val elapsed: Duration = to - from
    val minutes = elapsed.inWholeMinutes

    return when {
        minutes < 1 -> "under 1m"
        minutes < 60 -> "${minutes}m"
        else -> "${minutes / 60}h ${(minutes % 60).toString().padStart(2, '0')}m"
    }
}

fun runTokens(tokens: Long): String = when {
    tokens >= 1_000_000 -> "${tokens / 100_000 / 10.0}M"
    tokens >= 1_000 -> "${tokens / 1_000}k"
    else -> tokens.toString()
}

fun Workflow.shownSteps(round: Int): List<WorkflowRunStep> = if (round >= this.round) steps else rounds.firstOrNull { it.round == round }?.steps.orEmpty()

fun Workflow.earlierRounds(round: Int): List<WorkflowRound> = rounds.filter { it.round < round }.sortedBy { it.round }

fun Workflow.decisionFor(round: Int, step: String): WorkflowDecision? = decisions.lastOrNull { it.round == round && it.step == step }

fun Workflow.roundStarted(round: Int): WorkflowRound? = rounds.firstOrNull { it.round == round }

fun Workflow.firstStepTitle(): String = plan.steps.firstOrNull()?.title.orEmpty()

fun Workflow.lastRound(): Boolean = round >= plan.maxRounds

fun Workflow.waitingFor(step: String, steps: List<WorkflowRunStep>): String? {
    val spec = plan.steps.firstOrNull { it.key == step } ?: return null
    val pending = spec.dependsOn.firstOrNull { dependency -> steps.firstOrNull { it.key == dependency }?.status.let { it != WorkflowStepStatuses.Done && it != WorkflowStepStatuses.Approved } }

    return pending?.let { stepTitle(it) }
}

fun Workflow.failedBecause(step: String, steps: List<WorkflowRunStep>): String? {
    val failed = endReason?.takeIf { it.reason == "failed" }?.step ?: return null
    val status = steps.firstOrNull { it.key == step }?.status

    return if (step != failed && (status == null || status == WorkflowStepStatuses.NotRun || status == WorkflowStepStatuses.Waiting)) stepTitle(failed) else null
}

fun stepMeta(spec: WorkflowStep, nodeName: String?): String = when {
    spec.gate -> "Gate: waits for a decision"
    spec.workflow != null -> "Nested workflow: ${spec.workflow.title}"
    else -> listOfNotNull(spec.agent?.let(::agentName) ?: if (spec.providerId != null) "Chat" else null, nodeName, spec.model).joinToString(" · ")
}

fun Workflow.subagentConversation(conversations: Collection<Conversation>, agentId: String): Conversation? =
    conversations.firstOrNull { it.parentId == conversationId && it.agentId == agentId }

fun WorkflowRunStep.asAgent(): WorkflowAgent = WorkflowAgent(
    key = key,
    label = label ?: key,
    phase = phase.orEmpty(),
    finished = status == WorkflowStepStatuses.Done,
    tokens = tokens,
    turns = turns,
    elapsedMs = elapsedMs,
    agentId = agentId,
    failed = status == WorkflowStepStatuses.Failed,
    paused = status == WorkflowStepStatuses.Paused,
)

fun Workflow.agentPhases(): List<String> = (phases + steps.mapNotNull { it.phase }).distinct()
