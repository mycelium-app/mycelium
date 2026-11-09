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
