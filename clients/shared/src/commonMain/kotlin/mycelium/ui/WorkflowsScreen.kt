package mycelium.ui

import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import mycelium.api.ModelProvider
import mycelium.api.Workflow
import mycelium.api.WorkflowControlRequest
import mycelium.api.WorkflowKinds
import mycelium.api.WorkflowPlan
import mycelium.api.WorkflowStatuses
import mycelium.api.WorkflowStep
import mycelium.state.AppState
import mycelium.state.Choice
import mycelium.state.Store
import mycelium.state.WorkflowActions
import mycelium.state.WorkflowFilters
import mycelium.state.ended
import mycelium.state.stalled
import mycelium.state.sessionOptions
import mycelium.state.visibleWorkflows
import mycelium.state.workflowInGroup
import mycelium.state.workflowWhere

private data class WorkflowDraft(
    val title: String = "",
    val prompt: String = "",
    val node: String = "",
    val cwd: String = "",
    val projectId: String = "",
    val branch: String = "",
    val chat: Boolean = false,
    val providerId: String = "",
    val agent: String = "codex",
    val model: String = "",
    val difficulty: String = "",
    val dependsOn: Set<Int> = emptySet(),
    val gate: Boolean = false,
    val nested: WorkflowPlan? = null,
)

private const val WorkflowsList = "workflows"

private val ColumnWeights = listOf(3f, 2f, 2.4f, 2.4f, 1.4f, 1.2f)

@Composable
fun WorkflowsScreen(store: Store, state: AppState, onOpenConversation: (String) -> Unit, onOpenWorkflow: (String) -> Unit) {
    val colors = MyTheme.colors
    val group = LocalGroupFilter.current
    val workflows = state.workflows.values.filter { workflowInGroup(it, state, group) }
    var query by remember { mutableStateOf("") }
    var filter by remember { mutableStateOf(WorkflowFilters.All) }
    var projectId by remember { mutableStateOf<String?>(null) }
    var loaded by remember { mutableStateOf(false) }
    var creating by remember { mutableStateOf(false) }
    var stopping by remember { mutableStateOf<Workflow?>(null) }
    val loadFailures by store.loadFailures.collectAsState()
    val load = { store.load(WorkflowsList) { store.refreshWorkflows(); loaded = true } }
    val runner = rememberAction()
    val clock = workflowClock()
    val now = rememberNow(30_000L)
    val projects = workflows.mapNotNull { it.projectId }.distinct().mapNotNull { id -> state.projects[id]?.let { Choice(it.id, it.name) } }
    val shown = visibleWorkflows(workflows, filter, query, projectId)

    LaunchedEffect(store) {
        load()
    }

    Column(Modifier.fillMaxSize().background(colors.bgContent)) {
        Bar {
            OpenListButton()
            BarTitle("Workflows")
            Box(Modifier.weight(1f))
            MyButton({ creating = !creating }, text = "New workflow", icon = MyIcon.Plus, variant = ButtonVariant.Outline)
        }

        Column(Modifier.fillMaxWidth().weight(1f).verticalScrollWithBar().padding(Tokens.Space.S4), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
            runner.failure?.let { ErrorBanner(it, onDismiss = { runner.failure = null }) }

            if (creating) {
                NewWorkflowForm(store, state, workflows) { id ->
                    creating = false
                    onOpenWorkflow(id)
                }
            } else if (WorkflowsList in loadFailures && workflows.isEmpty()) {
                LoadFailedState("workflows", load)
            } else if (loaded && state.workflows.isEmpty()) {
                Column(Modifier.fillMaxWidth().padding(vertical = Tokens.Space.S8), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
                    Icon(MyIcon.Workflow, colors.textMuted, Tokens.Size.IconMd)
                    Label("No workflows yet", Modifier.semantics { heading() }, Tokens.Type.Title)
                    Label("A workflow runs several agents to a plan: each step is a conversation, and a gate waits for your decision. Ask an agent to start one, or start one here.", color = colors.textMuted)
                    MyButton({ creating = true }, text = "New workflow", icon = MyIcon.Plus, variant = ButtonVariant.Outline)
                }
            } else if (workflows.isNotEmpty()) {
                FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2), itemVerticalAlignment = Alignment.CenterVertically) {
                    ListSearch(query, { query = it }, "workflows")
                    Segmented(WorkflowFilters.Options, filter, { filter = it }, groupLabel = "Status")
                    ProjectFilter(projects, projectId) { projectId = it }
                }

                if (shown.isEmpty()) {
                    EmptyState("No workflow matches.", Modifier.fillMaxWidth().padding(vertical = Tokens.Space.S4))
                } else {
                    WorkflowHeader()

                    for (workflow in shown) {
                        WorkflowRow(
                            workflow = workflow,
                            state = state,
                            stalled = workflow.stalled(now),
                            clock = clock,
                            onOpen = { onOpenWorkflow(workflow.id) },
                            onOpenConversation = onOpenConversation,
                            onControl = { action -> runner { store.controlWorkflow(workflow.id, WorkflowControlRequest(action)) } },
                            onStop = { stopping = workflow },
                            onRerun = { runner { onOpenWorkflow(store.rerunWorkflow(workflow.id)) } },
                        )
                    }
                }
            }
        }
    }

    stopping?.let { workflow ->
        StopWorkflowDialog(workflow.title, { stopping = null }) {
            stopping = null
            runner { store.controlWorkflow(workflow.id, WorkflowControlRequest(WorkflowActions.Stop)) }
        }
    }
}

@Composable
private fun WorkflowHeader() {
    val colors = MyTheme.colors

    Row(Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S2), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        for ((index, name) in listOf("Workflow", "Status", "Where it is", "Started from", "Project", "Started").withIndex()) {
            Label(name, Modifier.weight(ColumnWeights[index]), Tokens.Type.MetaStrong, colors.textMuted, maxLines = 1)
        }

        Box(Modifier.width(Tokens.Size.ControlSm))
    }
    Hairline()
}

@Composable
private fun WorkflowRow(
    workflow: Workflow,
    state: AppState,
    stalled: Boolean,
    clock: (String?) -> String,
    onOpen: () -> Unit,
    onOpenConversation: (String) -> Unit,
    onControl: (String) -> Unit,
    onStop: () -> Unit,
    onRerun: () -> Unit,
) {
    val colors = MyTheme.colors
    val menu = rememberRowMenuState()
    val conversation = workflow.conversationId?.let { state.conversations[it] }
    val schedule = workflow.startedBy?.takeIf { it.kind == "schedule" }
    val ended = workflow.ended()
    val paused = workflow.status == WorkflowStatuses.Paused
    val mycelium = workflow.kind == WorkflowKinds.Mycelium

    Row(Modifier.fillMaxWidth().rowContextMenu(menu).padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
        Row(
            Modifier.weight(ColumnWeights[0]).pressable(onOpen, shape = BlockShape, description = "Open ${workflow.title}").padding(vertical = Tokens.Space.S1),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(MyIcon.Workflow, colors.textMuted, Tokens.Size.IconSm)
            Label(workflow.title, style = Tokens.Type.UiStrong, maxLines = 1)
        }

        WorkflowStatusMark(workflowStatusLook(workflow.status, stalled), Modifier.weight(ColumnWeights[1]))
        Label(workflowWhere(workflow), Modifier.weight(ColumnWeights[2]), Tokens.Type.Meta, colors.textMuted, maxLines = 1)

        Row(Modifier.weight(ColumnWeights[3]), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
            if (conversation != null) {
                MyButton({ onOpenConversation(conversation.id) }, text = conversation.title, description = "Open the conversation ${conversation.title}", size = ButtonSize.Sm, muted = true)
            } else if (schedule != null) {
                Icon(MyIcon.Calendar, colors.textMuted, Tokens.Size.IconSm)
                Label(schedule.name, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
            } else {
                Label("Workflows page", style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
            }
        }

        Label(workflow.projectId?.let { state.projects[it]?.name }.orEmpty(), Modifier.weight(ColumnWeights[4]), Tokens.Type.Meta, colors.textMuted, maxLines = 1)
        Label(clock(workflow.createdAt), Modifier.weight(ColumnWeights[5]), Tokens.Type.Meta, colors.textMuted, maxLines = 1)

        RowMenu(workflow.title, menu) { close ->
            MenuItem("Open", onClick = { close(); onOpen() }, leading = MyIcon.Workflow)

            if (conversation != null) {
                MenuItem("Open the starting conversation", onClick = { close(); onOpenConversation(conversation.id) }, leading = MyIcon.Chat)
            }

            if (!ended && mycelium) {
                MenuItem(if (paused) "Resume" else "Pause", onClick = { close(); onControl(if (paused) WorkflowActions.Resume else WorkflowActions.Pause) }, leading = MyIcon.Clock)
            }

            if (!ended) {
                MenuItem("Stop…", onClick = { close(); onStop() }, leading = MyIcon.Stop)
            }

            if (ended && mycelium) {
                MenuItem("Re-run", onClick = { close(); onRerun() }, leading = MyIcon.Refresh)
            }
        }
    }
    Hairline()
}

@Composable
private fun NewWorkflowForm(store: Store, state: AppState, existing: List<Workflow>, onStarted: (String) -> Unit) {
    val colors = MyTheme.colors
    val runner = rememberAction()
    var providers by remember { mutableStateOf<List<ModelProvider>>(emptyList()) }
    var title by remember { mutableStateOf("") }
    var drafts by remember { mutableStateOf(listOf(WorkflowDraft(node = state.nodes.keys.firstOrNull().orEmpty()))) }
    var rounds by remember { mutableStateOf("1") }
    var stopWhen by remember { mutableStateOf("") }

    LaunchedEffect(store) {
        store.act {
            providers = store.api.modelProviders()
            store.refreshProjects()
        }
    }

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
        runner.failure?.let { ErrorBanner(it, onDismiss = { runner.failure = null }) }
        Label("New workflow", style = Tokens.Type.MetaStrong)
        WorkflowField("Title", title, { title = it })
        WorkflowField("Maximum rounds (1 to 20)", rounds, { rounds = it })
        WorkflowField("Stop when every result contains (optional)", stopWhen, { stopWhen = it })
        for ((index, draft) in drafts.withIndex()) {
            Column(Modifier.fillMaxWidth().background(colors.bgPanel, BlockShape).border(Tokens.Size.Hairline, colors.border, BlockShape).padding(Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Label("Step ${index + 1}", style = Tokens.Type.MetaStrong)
                WorkflowField("Step ${index + 1} title", draft.title, { value -> drafts = drafts.toMutableList().also { it[index] = draft.copy(title = value) } })
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    MyButton({ drafts = drafts.toMutableList().also { it[index] = draft.copy(gate = false, nested = null) } }, text = "Agent", selected = !draft.gate && draft.nested == null)
                    MyButton({ drafts = drafts.toMutableList().also { it[index] = draft.copy(gate = true, nested = null) } }, text = "Human decision", selected = draft.gate)
                }

                if (!draft.gate && draft.nested == null) {
                    WorkflowField("Step ${index + 1} prompt", draft.prompt, { value -> drafts = drafts.toMutableList().also { it[index] = draft.copy(prompt = value) } }, multiline = true)
                    Label("Use {{step1.output}} to include another step’s result, or {{previous}} for the previous round.", color = colors.textMuted)
                    Segmented(listOf("Workspace agent", "Chat model"), if (draft.chat) "Chat model" else "Workspace agent", { value -> drafts = drafts.toMutableList().also { it[index] = draft.copy(chat = value == "Chat model", model = "", difficulty = "") } }, groupLabel = "Step ${index + 1} runtime")

                    if (!draft.chat) {
                    WorkflowField("Step ${index + 1} folder", draft.cwd, { value -> drafts = drafts.toMutableList().also { it[index] = draft.copy(cwd = value) } })
                    for (project in state.projects.values.filter { it.nodeId == draft.node }) {
                        MyButton({ drafts = drafts.toMutableList().also { it[index] = draft.copy(projectId = project.id, cwd = project.path) } }, text = project.name, selected = draft.projectId == project.id)
                    }

                    if (draft.projectId.isNotBlank()) {
                        WorkflowField("Step ${index + 1} worktree branch (optional)", draft.branch, { value -> drafts = drafts.toMutableList().also { it[index] = draft.copy(branch = value) } })
                    }
                    Label("Node", color = colors.textMuted)
                    for ((nodeId, availableNode) in state.nodes) {
                        MyButton({ drafts = drafts.toMutableList().also { it[index] = draft.copy(node = nodeId) } }, text = availableNode.hostname, selected = draft.node == nodeId)
                    }

                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        for (agent in listOf("claude", "codex")) {
                            MyButton({ drafts = drafts.toMutableList().also { it[index] = draft.copy(agent = agent) } }, text = if (agent == "claude") "Claude" else "Codex", selected = draft.agent == agent)
                        }
                    }

                    val available = state.conversations.values.filter { it.nodeId == draft.node && it.agent == draft.agent }.map { sessionOptions(it, emptyList()) }
                    val models = available.flatMap { it.models }.distinctBy { it.id }
                    val difficulties = available.flatMap { it.settings }.filter { it.category == "thought_level" || it.id.contains("effort") || it.id.contains("reasoning") }.flatMap { it.choices }.distinctBy { it.id }
                    Label("Model", color = colors.textMuted)
                    MyButton({ drafts = drafts.toMutableList().also { it[index] = draft.copy(model = "") } }, text = "Agent default", selected = draft.model.isBlank())
                    for (model in models) {
                        MyButton({ drafts = drafts.toMutableList().also { it[index] = draft.copy(model = model.id) } }, text = model.name, selected = draft.model == model.id)
                    }

                    Label("Difficulty", color = colors.textMuted)
                    MyButton({ drafts = drafts.toMutableList().also { it[index] = draft.copy(difficulty = "") } }, text = "Agent default", selected = draft.difficulty.isBlank())
                    for (difficulty in difficulties) {
                        MyButton({ drafts = drafts.toMutableList().also { it[index] = draft.copy(difficulty = difficulty.id) } }, text = difficulty.name, selected = draft.difficulty == difficulty.id)
                    }
                    } else {
                        if (providers.isEmpty()) {
                            Label("Add a chat model provider in Settings before starting this step.", color = colors.textMuted)
                        }

                        for (provider in providers) {
                            MyButton({ drafts = drafts.toMutableList().also { it[index] = draft.copy(providerId = provider.id, model = provider.defaultModel.orEmpty()) } }, text = provider.name, selected = draft.providerId == provider.id)
                        }

                        val provider = providers.firstOrNull { it.id == draft.providerId }

                        for (model in provider?.models.orEmpty()) {
                            MyButton({ drafts = drafts.toMutableList().also { it[index] = draft.copy(model = model) } }, text = model, selected = draft.model == model)
                        }
                    }
                }

                if (draft.nested != null) {
                    Label("Nested workflow: ${draft.nested.title}")
                }

                if (existing.isNotEmpty()) {
                    Label("Or run an existing workflow", color = colors.textMuted)
                    for (nested in existing.filter { it.parentId == null }) {
                        MyButton({ drafts = drafts.toMutableList().also { it[index] = draft.copy(gate = false, nested = nested.plan) } }, text = nested.title)
                    }
                }

                if (index > 0) {
                    Label("Wait for", color = colors.textMuted)
                    for (predecessor in 0 until index) {
                        MyButton({
                            val dependencies = if (predecessor in draft.dependsOn) draft.dependsOn - predecessor else draft.dependsOn + predecessor
                            drafts = drafts.toMutableList().also { it[index] = draft.copy(dependsOn = dependencies) }
                        }, text = drafts[predecessor].title.ifBlank { "Step ${predecessor + 1}" }, selected = predecessor in draft.dependsOn)
                    }
                }
            }
        }

        MyButton({ drafts = drafts + WorkflowDraft(node = state.nodes.keys.firstOrNull().orEmpty()) }, text = "Add step", icon = MyIcon.Plus)
        MyButton({
            runner {
                val plan = WorkflowPlan(
                    title = title,
                    maxRounds = rounds.toIntOrNull() ?: 0,
                    stopWhen = stopWhen.takeIf { it.isNotBlank() },
                    steps = drafts.mapIndexed { index, draft ->
                        val runsAgent = draft.nested == null && !draft.gate
                        val inWorkspace = runsAgent && !draft.chat

                        WorkflowStep(
                            key = "step${index + 1}",
                            title = draft.title,
                            prompt = draft.prompt,
                            gate = draft.gate,
                            dependsOn = draft.dependsOn.sorted().map { "step${it + 1}" },
                            workflow = draft.nested,
                            providerId = draft.providerId.takeIf { runsAgent && draft.chat },
                            nodeId = draft.node.takeIf { inWorkspace },
                            agent = draft.agent.takeIf { inWorkspace },
                            cwd = draft.cwd.takeIf { inWorkspace },
                            projectId = draft.projectId.takeIf { inWorkspace && it.isNotBlank() },
                            branch = draft.branch.takeIf { inWorkspace && it.isNotBlank() },
                            model = draft.model.takeIf { runsAgent && it.isNotBlank() },
                            difficulty = draft.difficulty.takeIf { runsAgent && it.isNotBlank() },
                        )
                    },
                )
                val workflow = store.api.createWorkflow(plan)
                store.refreshWorkflows()
                onStarted(workflow.id)
            }
        }, text = "Start workflow", variant = ButtonVariant.Outline)
    }
}

@Composable
private fun WorkflowField(label: String, value: String, onChange: (String) -> Unit, multiline: Boolean = false) {
    val colors = MyTheme.colors
    if (!multiline) {
        TextInput(value, onChange, label)
        return
    }

    val source = remember { MutableInteractionSource() }
    val focused = source.collectIsFocusedAsState()
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Label(label, color = colors.textMuted)
        BasicTextField(value, onChange, interactionSource = source, modifier = Modifier.fillMaxWidth().heightIn(min = 80.dp).focusRing(focused, ControlShape, always = true).background(colors.bgPanel).border(1.dp, colors.textMuted).padding(8.dp).semantics { contentDescription = label }, textStyle = Tokens.Type.MetaStrong.copy(color = colors.text), singleLine = false)
    }
}
