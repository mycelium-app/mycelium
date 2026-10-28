package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.AnnotatedString
import mycelium.api.MyceliumJson
import mycelium.api.Workflow
import mycelium.api.WorkflowControlRequest
import mycelium.api.WorkflowKinds
import mycelium.api.WorkflowPlan
import mycelium.api.WorkflowRunStep
import mycelium.api.WorkflowStatuses
import mycelium.api.WorkflowStep
import mycelium.state.AppState
import mycelium.state.Store
import mycelium.state.WorkflowActions
import mycelium.state.WorkflowStepStatuses
import mycelium.state.agentPhases
import mycelium.state.asAgent
import mycelium.state.decisionFor
import mycelium.state.earlierRounds
import mycelium.state.ended
import mycelium.state.failedBecause
import mycelium.state.roundStarted
import mycelium.state.runMinutes
import mycelium.state.runTokens
import mycelium.state.shownSteps
import mycelium.state.stalled
import mycelium.state.stepMeta
import mycelium.state.stepTitle
import mycelium.state.subagentConversation
import mycelium.state.waitingFor
import mycelium.state.workflowStats

private const val WorkflowTickMillis = 30_000L

private fun workflowLoadKey(id: String) = "workflow:$id"

@Composable
fun WorkflowScreen(
    store: Store,
    state: AppState,
    id: String,
    onBack: () -> Unit,
    onOpenConversation: (String) -> Unit,
    onOpenWorkflow: (String) -> Unit,
    onOpenProjects: () -> Unit = {},
) {
    val colors = MyTheme.colors
    val loadFailures by store.loadFailures.collectAsState()
    val clock = workflowClock()
    val runner = rememberAction(id)
    val clipboard = LocalClipboardManager.current
    val now = rememberNow(WorkflowTickMillis)
    var viewedRound by remember(id) { mutableStateOf<Int?>(null) }
    var stopping by remember(id) { mutableStateOf(false) }
    var choice by remember(id) { mutableStateOf<GateChoice?>(null) }
    val menu = rememberRowMenuState()

    LaunchedEffect(id) {
        store.load(workflowLoadKey(id)) { store.refreshWorkflow(id) }
    }

    val workflow = state.workflows[id]

    if (workflow == null) {
        Column(Modifier.fillMaxSize().background(colors.bgContent)) {
            Bar {
                BackButton("workflows", onBack)
                Label(if (workflowLoadKey(id) in loadFailures) "Workflow not found" else "Workflow", style = Tokens.Type.Title, maxLines = 1)
            }

            if (workflowLoadKey(id) in loadFailures) {
                Label("This workflow no longer exists or is not available to you.", Modifier.padding(Tokens.Space.S4), color = colors.textMuted)
            }
        }

        return
    }

    val agentRun = workflow.kind == WorkflowKinds.AgentRun
    val stalled = workflow.stalled(now)
    val look = workflowStatusLook(workflow.status, stalled)
    val ended = workflow.ended()
    val round = (viewedRound ?: workflow.round).coerceIn(1, maxOf(workflow.round, 1))
    val live = round == workflow.round && !ended
    val conversation = workflow.conversationId?.let { state.conversations[it] }
    val project = workflow.projectId?.let { state.projects[it] }
    val canRerun = !agentRun && ended

    Column(Modifier.fillMaxSize().background(colors.bgContent)) {
        Bar {
            BackButton("workflows", onBack)
            Icon(MyIcon.Workflow, colors.textMuted, Tokens.Size.IconSm)
            Label(workflow.title, Modifier.semantics { heading() }, Tokens.Type.UiStrong, maxLines = 1)

            Row(Modifier.weight(1f), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
                if (project != null) {
                    MyButton(onOpenProjects, text = project.name, icon = MyIcon.Folder, description = "Project: ${project.name}", size = ButtonSize.Sm, muted = true)
                }

                if (project != null && conversation != null) {
                    Label("/", style = Tokens.Type.Meta, color = colors.textFaint)
                }

                if (conversation != null) {
                    MyButton({ onOpenConversation(conversation.id) }, text = conversation.title, icon = MyIcon.Chat, description = "Open the conversation ${conversation.title}", size = ButtonSize.Sm, muted = true)
                }
            }

            WorkflowStatusMark(look)

            if (!agentRun && !ended) {
                val paused = workflow.status == WorkflowStatuses.Paused

                MyButton(
                    { runner { store.controlWorkflow(id, WorkflowControlRequest(if (paused) WorkflowActions.Resume else WorkflowActions.Pause)) } },
                    text = if (paused) "Resume" else "Pause",
                    icon = if (paused) MyIcon.Working else MyIcon.Clock,
                    variant = ButtonVariant.Outline,
                    size = ButtonSize.Sm,
                )
            }

            if (!ended) {
                MyButton({ stopping = true }, text = "Stop", icon = MyIcon.Stop, variant = ButtonVariant.Outline, size = ButtonSize.Sm)
            }

            if (canRerun && workflow.status != WorkflowStatuses.Done) {
                MyButton({ runner { onOpenWorkflow(store.rerunWorkflow(id)) } }, text = "Re-run", icon = MyIcon.Refresh, variant = ButtonVariant.Primary, size = ButtonSize.Sm)
            }

            if (!agentRun) {
                RowMenu(workflow.title, menu) { close ->
                    if (canRerun && workflow.status == WorkflowStatuses.Done) {
                        MenuItem("Re-run", onClick = { close(); runner { onOpenWorkflow(store.rerunWorkflow(id)) } }, leading = MyIcon.Refresh)
                    }

                    MenuItem("Copy plan as JSON", onClick = { close(); clipboard.setText(AnnotatedString(MyceliumJson.encodeToString(WorkflowPlan.serializer(), workflow.plan))) }, leading = MyIcon.Copy)
                }
            }
        }

        Column(Modifier.weight(1f).fillMaxWidth().verticalScrollWithBar().padding(Tokens.Space.S4), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
            runner.failure?.let { ErrorBanner(it, onDismiss = { runner.failure = null }) }

            if (agentRun) {
                AgentRunBody(workflow, state, clock, now, onOpenConversation)
            } else {
                RoundToolbar(workflow, round, clock) { viewedRound = it }
                EndNotice(workflow, clock)

                val steps = workflow.shownSteps(round)

                for ((index, spec) in workflow.plan.steps.withIndex()) {
                    StepLane(
                        workflow = workflow,
                        index = index,
                        spec = spec,
                        step = steps.firstOrNull { it.key == spec.key },
                        steps = steps,
                        round = round,
                        live = live,
                        state = state,
                        clock = clock,
                        onOpenConversation = onOpenConversation,
                        onOpenWorkflow = onOpenWorkflow,
                        onDecide = { choice = it },
                    )
                }
            }
        }
    }

    if (stopping) {
        StopWorkflowDialog(workflow.title, { stopping = false }) {
            stopping = false
            runner { store.controlWorkflow(id, WorkflowControlRequest(WorkflowActions.Stop)) }
        }
    }

    choice?.let { chosen ->
        GateDecisionDialog(workflow, chosen, { choice = null }) { request ->
            choice = null
            runner { store.controlWorkflow(id, request) }
        }
    }
}

@Composable
private fun RoundToolbar(workflow: Workflow, round: Int, clock: (String?) -> String, onRound: (Int) -> Unit) {
    val colors = MyTheme.colors
    val plan = workflow.plan
    val rule = workflow.stopRule?.takeIf { it.isNotBlank() } ?: plan.stopWhen?.takeIf { it.isNotBlank() }?.let { "every result contains $it" }
    val who = workflow.startedBy?.let { if (it.kind == "schedule") " by the schedule ${it.name}" else " by ${it.name}" }.orEmpty()
    val started = if (workflow.createdAt.isBlank()) "" else " Started ${clock(workflow.createdAt)}$who."
    val rules = "Up to ${plan.maxRounds} ${if (plan.maxRounds == 1) "round" else "rounds"}." + (rule?.let { " Ends early when $it." } ?: "") + started
    val rounds = maxOf(workflow.round, workflow.rounds.maxOfOrNull { it.round } ?: 1)

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        if (rounds > 1) {
            Segmented((1..rounds).map { "Round $it" }, "Round $round", { onRound(it.removePrefix("Round ").toInt()) }, groupLabel = "Round")
        }

        Label(rules, style = Tokens.Type.Meta, color = colors.textMuted)

        if (round > 1) {
            val note = roundNote(workflow, round, clock)

            if (note.isNotBlank()) {
                Label(note, style = Tokens.Type.Meta, color = colors.textMuted)
            }
        }
    }
}

private fun roundNote(workflow: Workflow, round: Int, clock: (String?) -> String): String {
    val started = workflow.roundStarted(round)
    val sentBack = workflow.decisions.lastOrNull { it.round == round - 1 && it.decision == "send_back" }
    val opening = "Round $round started${started?.startedAt?.let { " at ${clock(it)}" }.orEmpty()}."

    if (sentBack != null) {
        val reason = sentBack.reason?.takeIf { it.isNotBlank() }?.let { ": “$it”" } ?: "."

        return "$opening ${sentBack.by.name} sent round ${round - 1} back at ${workflow.stepTitle(sentBack.step)}$reason Every step ran again with round ${round - 1}’s results."
    }

    return listOfNotNull(opening, started?.reason?.takeIf { it.isNotBlank() }).joinToString(" ")
}

@Composable
private fun EndNotice(workflow: Workflow, clock: (String?) -> String) {
    val reason = workflow.endReason ?: return

    when (reason.reason) {
        "failed" -> {
            val step = workflow.stepTitle(reason.step)
            val failed = workflow.steps.firstOrNull { it.key == reason.step }
            val hint = if (failed?.conversationId != null) " Open $step’s conversation to see the error, then re-run." else ""
            val error = (reason.error?.takeIf { it.isNotBlank() } ?: failed?.error ?: "no error was reported").trimEnd('.')

            Callout(MyIcon.Failed, Tone.Failed, "$step failed in round ${reason.round ?: workflow.round}: $error.$hint")
        }
        "stopped" -> {
            val at = workflow.endedAt?.let { " at ${clock(it)}" }.orEmpty()
            val who = reason.by?.name?.let { " by $it" }.orEmpty()
            val running = workflow.steps.filter { it.status == WorkflowStepStatuses.Stopped }.map { workflow.stepTitle(it.key) }
            val left = workflow.steps.filter { it.status == WorkflowStepStatuses.NotRun || it.status == WorkflowStepStatuses.Waiting }.map { workflow.stepTitle(it.key) }
            val detail = listOfNotNull(
                running.takeIf { it.isNotEmpty() }?.let { "${it.joinToString(" and ")} was running and stopped where it was." },
                left.takeIf { it.isNotEmpty() }?.let { "${it.joinToString(" and ")} did not run." },
            ).joinToString(" ").let { if (it.isEmpty()) "" else " $it" }

            Callout(MyIcon.Stop, Tone.Muted, "Stopped$who$at during round ${reason.round ?: workflow.round}.$detail")
        }
        "stop_rule" -> Callout(MyIcon.Check, Tone.Muted, "Ended early in round ${reason.round ?: workflow.round}: the stop rule was met.")
        "round_limit" -> Callout(MyIcon.Check, Tone.Muted, "Ended at the last round, round ${reason.round ?: workflow.round}.")
    }
}

@Composable
private fun Callout(icon: MyIcon, tone: Tone, text: String) {
    val colors = MyTheme.colors
    val accent = if (tone == Tone.Failed) colors.statusFailed else colors.textMuted

    Row(
        Modifier
            .fillMaxWidth()
            .background(colors.bgPanel, BlockShape)
            .border(Tokens.Size.Hairline, accent, BlockShape)
            .padding(Tokens.Space.S3)
            .semantics { liveRegion = LiveRegionMode.Polite },
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
        verticalAlignment = Alignment.Top,
    ) {
        Icon(icon, accent, Tokens.Size.IconSm)
        Label(text, Modifier.weight(1f))
    }
}

@Composable
private fun LaneHead(number: Int, title: String, meta: String?, uses: List<String>, status: @Composable () -> Unit) {
    val colors = MyTheme.colors

    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
        Box(Modifier.size(Tokens.Size.IconMd).border(Tokens.Size.Hairline, colors.border, ControlShape), contentAlignment = Alignment.Center) {
            Label("$number", style = Tokens.Type.MetaStrong, color = colors.textMuted)
        }

        Column(Modifier.weight(1f)) {
            Label(title, Modifier.semantics { heading() }, Tokens.Type.UiStrong)

            if (!meta.isNullOrBlank()) {
                Label(meta, style = Tokens.Type.Meta, color = colors.textMuted)
            }

            if (uses.isNotEmpty()) {
                Label("Uses ${uses.joinToString()}", style = Tokens.Type.Meta, color = colors.textMuted)
            }
        }

        status()
    }
}

@Composable
private fun InlineFact(name: String, value: String) {
    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        Label(name, style = Tokens.Type.Meta, color = MyTheme.colors.textMuted)
        Label(value, style = Tokens.Type.Meta)
    }
}

@Composable
private fun StepLane(
    workflow: Workflow,
    index: Int,
    spec: WorkflowStep,
    step: WorkflowRunStep?,
    steps: List<WorkflowRunStep>,
    round: Int,
    live: Boolean,
    state: AppState,
    clock: (String?) -> String,
    onOpenConversation: (String) -> Unit,
    onOpenWorkflow: (String) -> Unit,
    onDecide: (GateChoice) -> Unit,
) {
    val colors = MyTheme.colors
    val status = step?.status ?: if (workflow.ended()) WorkflowStepStatuses.NotRun else WorkflowStepStatuses.Waiting
    val decision = workflow.decisionFor(round, spec.key)
    val look = stepStatusLook(status, workflow.waitingFor(spec.key, steps))
    val because = workflow.failedBecause(spec.key, steps)
    val ran = step?.startedAt?.let { start ->
        val end = step?.endedAt
        val minutes = runMinutes(start, end)

        if (end == null) "since ${clock(start)}" else "${clock(start)} to ${clock(end)}${minutes?.let { ", $it" }.orEmpty()}"
    }
    val earlier = workflow.earlierRounds(round)

    Column(
        Modifier
            .fillMaxWidth()
            .background(colors.bgPanel, BlockShape)
            .border(Tokens.Size.Hairline, if (status == WorkflowStepStatuses.Failed) colors.statusFailed else colors.border, BlockShape)
            .padding(Tokens.Space.S3),
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
    ) {
        LaneHead(index + 1, spec.title, stepMeta(spec, spec.nodeId?.let { state.nodes[it]?.name }), spec.dependsOn.map { workflow.stepTitle(it) }) {
            WorkflowStatusMark(look)
        }

        if (status == WorkflowStepStatuses.NotRun || (because != null && status == WorkflowStepStatuses.Waiting)) {
            Label(if (because != null) "Did not run because $because failed." else "Did not run.", style = Tokens.Type.Meta, color = colors.textMuted)
        }

        if (ran != null) {
            InlineFact("Ran", ran)
        }

        step?.tokens?.let { InlineFact("Tokens", runTokens(it)) }

        val text = if (status == WorkflowStepStatuses.Failed) step?.error?.takeIf { it.isNotBlank() } ?: step?.summary.orEmpty() else step?.summary?.takeIf { it.isNotBlank() } ?: step?.output.orEmpty()

        if (text.isNotBlank() && status != WorkflowStepStatuses.Gate) {
            Label(text, maxLines = 4)
        }

        if (status == WorkflowStepStatuses.Gate) {
            InlineFact("Waiting since", clock(step?.startedAt ?: workflow.updatedAt))

            if (spec.prompt.isNotBlank()) {
                Label(spec.prompt)
            }

            if (live) {
                Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                    MyButton({ onDecide(GateChoice(spec.key, sendBack = false)) }, text = "Approve", description = "Approve ${spec.title}", variant = ButtonVariant.Primary, size = ButtonSize.Sm)
                    MyButton({ onDecide(GateChoice(spec.key, sendBack = true)) }, text = "Send back", description = "Send ${spec.title} back", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
                }
            }
        }

        if (decision != null) {
            val verb = if (decision.decision == WorkflowActions.SendBack) "Sent back" else "Approved"
            val reason = decision.reason?.takeIf { it.isNotBlank() }?.let { ": “$it”" }.orEmpty()

            Label("$verb by ${decision.by.name} at ${clock(decision.at)}$reason", style = Tokens.Type.Meta, color = colors.textMuted)
        }

        FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            step?.conversationId?.let { conversation ->
                MyButton({ onOpenConversation(conversation) }, text = "Open conversation", description = "Open the conversation for ${spec.title}", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
            }

            step?.workflowId?.let { child ->
                MyButton({ onOpenWorkflow(child) }, text = "Open nested workflow", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
            }
        }

        for (past in earlier) {
            EarlierRound(workflow, past.round, spec, past.steps.firstOrNull { it.key == spec.key }, past.startedAt, past.endedAt, clock)
        }
    }
}

@Composable
private fun EarlierRound(workflow: Workflow, round: Int, spec: WorkflowStep, step: WorkflowRunStep?, startedAt: String, endedAt: String?, clock: (String?) -> String) {
    val colors = MyTheme.colors
    var open by remember(workflow.id, round, spec.key) { mutableStateOf(false) }
    val decision = workflow.decisionFor(round, spec.key)
    val duration = runMinutes(step?.startedAt ?: startedAt, step?.endedAt ?: endedAt)
    val outcome = if (decision != null) {
        "${if (decision.decision == WorkflowActions.SendBack) "Sent back" else "Approved"} by ${decision.by.name} at ${clock(decision.at)}"
    } else {
        stepStatusLook(step?.status ?: WorkflowStepStatuses.NotRun).label + (duration?.let { ", $it" } ?: "")
    }
    val title = "Round $round: $outcome"

    MyButton(
        { open = !open },
        text = title,
        trailingIcon = if (open) MyIcon.ChevronDown else MyIcon.ChevronRight,
        size = ButtonSize.Sm,
        muted = true,
        stateDescription = if (open) "Expanded" else "Collapsed",
    )

    if (open) {
        val text = step?.summary?.takeIf { it.isNotBlank() } ?: step?.output.orEmpty()

        Label(text.ifBlank { "No result was recorded for this round." }, Modifier.padding(start = Tokens.Space.S3), color = colors.textMuted, maxLines = 4)
    }
}

@Composable
private fun AgentRunBody(workflow: Workflow, state: AppState, clock: (String?) -> String, now: kotlin.time.Instant, onOpenConversation: (String) -> Unit) {
    val colors = MyTheme.colors
    val conversation = workflow.conversationId?.let { state.conversations[it] }
    val tokens = workflow.steps.mapNotNull { it.tokens }.takeIf { it.isNotEmpty() }?.sum()
    val summary = listOfNotNull(
        "Run by Claude Code’s Workflow tool" + (conversation?.let { " in ${it.title}" } ?: "") + ".",
        workflow.createdAt.takeIf { it.isNotBlank() }?.let { "Started ${clock(it)}." },
        tokens?.let { "${runTokens(it)} tokens." },
    ).joinToString(" ")
    val failed = workflow.steps.firstOrNull { it.status == WorkflowStepStatuses.Failed }

    Label(summary, style = Tokens.Type.Meta, color = colors.textMuted)

    if (workflow.status == WorkflowStatuses.Failed) {
        val reason = workflow.endReason?.error ?: failed?.error ?: "no error was reported"

        Callout(MyIcon.Failed, Tone.Failed, "${failed?.label ?: "The workflow"} failed: $reason")
    }

    if (workflow.status == WorkflowStatuses.Stopped) {
        val at = workflow.endedAt?.let { " at ${clock(it)}" }.orEmpty()

        Callout(MyIcon.Stop, Tone.Muted, "Stopped${workflow.endReason?.by?.name?.let { " by $it" }.orEmpty()}$at.")
    }

    if (workflow.stalled(now)) {
        Callout(MyIcon.Alert, Tone.Muted, "No progress for ${(now - (kotlin.time.Instant.parse(workflow.updatedAt))).inWholeMinutes} minutes.")
    }

    val phases = workflow.agentPhases()

    for ((index, phase) in phases.withIndex()) {
        val agents = workflow.steps.filter { it.phase == phase }
        val finished = agents.count { it.status == WorkflowStepStatuses.Done }
        val status = when {
            agents.isEmpty() && workflow.ended() -> WorkflowStepStatuses.NotRun
            agents.isEmpty() -> WorkflowStepStatuses.Waiting
            agents.any { it.status == WorkflowStepStatuses.Failed } -> WorkflowStepStatuses.Failed
            agents.any { it.status == WorkflowStepStatuses.Paused } -> WorkflowStepStatuses.Paused
            finished == agents.size -> WorkflowStepStatuses.Done
            workflow.status == WorkflowStatuses.Stopped -> WorkflowStepStatuses.Stopped
            else -> WorkflowStepStatuses.Running
        }
        val before = phases.getOrNull(index - 1)
        val look = stepStatusLook(status, before)
        val label = if (status == WorkflowStepStatuses.Running) "${look.label}, $finished of ${agents.size} finished" else look.label

        Column(
            Modifier
                .fillMaxWidth()
                .background(colors.bgPanel, BlockShape)
                .border(Tokens.Size.Hairline, colors.border, BlockShape)
                .padding(Tokens.Space.S3),
            verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
        ) {
            LaneHead(index + 1, phase, "Claude Code" + if (agents.isNotEmpty()) " · ${agents.size} ${if (agents.size == 1) "agent" else "agents"}" else "", before?.let { listOf(it) }.orEmpty()) {
                WorkflowStatusMark(look, text = label)
            }

            for (agent in agents) {
                val open = agent.agentId?.let { workflow.subagentConversation(state.conversations.values, it) }
                val agentLook = stepStatusLook(agent.status)
                val stats = workflowStats(agent.asAgent())

                Row(
                    Modifier
                        .fillMaxWidth()
                        .then(if (open != null) Modifier.pressable({ onOpenConversation(open.id) }, shape = BlockShape, description = "Open ${agent.label ?: agent.key}") else Modifier)
                        .padding(vertical = Tokens.Space.S1)
                        .semantics(mergeDescendants = true) { contentDescription = "${agentLook.label}: ${agent.label ?: agent.key}" },
                    horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                    verticalAlignment = Alignment.Top,
                ) {
                    Icon(agentLook.icon, agentLook.tone.color(), Tokens.Size.IconSm)

                    Column(Modifier.weight(1f)) {
                        Label(agent.label ?: agent.key, maxLines = 1)

                        if (agent.summary.isNotBlank()) {
                            Label(agent.summary, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
                        }
                    }

                    stats?.let { Label(it, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1) }
                }
            }
        }
    }
}
