package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.layout.layout
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import mycelium.Platform
import mycelium.api.ConversationStatus
import mycelium.api.CreateTaskRequest
import mycelium.api.Task
import mycelium.api.TaskStatuses
import mycelium.state.AppState
import mycelium.state.BoardCard
import mycelium.state.BoardFilter
import mycelium.state.Lane
import mycelium.state.Store
import mycelium.state.acceptsAMessageNow
import mycelium.state.agentName
import mycelium.state.boardCards
import mycelium.state.collapsedLanes
import mycelium.state.elapsed
import mycelium.state.pendingPermission
import mycelium.state.savedLanes

private const val CollapsedPreference = "tasks-collapsed-lanes"
private const val TickMillis = 1_000L
private val LaneMinWidth = 260.dp
private val CollapsedLaneWidth = 40.dp
private val finishedStatuses = setOf(ConversationStatus.Review, ConversationStatus.Done, ConversationStatus.Failed)

private fun laneIcon(lane: Lane): MyIcon = when (lane) {
    Lane.Questions -> MyIcon.Question
    Lane.Proposed -> MyIcon.Review
    Lane.Open -> MyIcon.List
    Lane.Claimed -> MyIcon.Working
    Lane.Done -> MyIcon.Check
}

@Composable
private fun laneColour(lane: Lane) = when (lane) {
    Lane.Questions, Lane.Proposed -> MyTheme.colors.statusAttention
    Lane.Claimed -> MyTheme.colors.statusWorking
    Lane.Open, Lane.Done -> MyTheme.colors.textMuted
}

private fun Modifier.vertical(): Modifier =
    layout { measurable, constraints ->
        val placeable = measurable.measure(constraints)

        layout(placeable.height, placeable.width) {
            placeable.place(x = -(placeable.width / 2 - placeable.height / 2), y = -(placeable.height / 2 - placeable.width / 2))
        }
    }.rotate(90f)

@Composable
fun TasksScreen(store: Store, state: AppState, platform: Platform, onOpen: (String) -> Unit) {
    val colors = MyTheme.colors
    var filter by remember { mutableStateOf(BoardFilter()) }
    var collapsed by remember { mutableStateOf(collapsedLanes(platform.loadPreference(CollapsedPreference))) }
    var projectMenu by remember { mutableStateOf(false) }
    var nodeMenu by remember { mutableStateOf(false) }
    var agentMenu by remember { mutableStateOf(false) }
    var adding by remember { mutableStateOf(false) }
    var answering by remember { mutableStateOf<Task?>(null) }
    var now by remember { mutableStateOf(kotlin.time.Clock.System.now()) }

    LaunchedEffect(Unit) {
        store.act { store.refreshBoard() }
    }

    val lanes = boardCards(state, filter, LocalGroupFilter.current)
    val claimed = lanes.getValue(Lane.Claimed)
    val waiting = claimed.mapNotNull { it.conversation?.takeIf { conversation -> conversation.status == ConversationStatus.Input }?.id }

    LaunchedEffect(waiting) {
        for (id in waiting) {
            store.act { store.open(id) }
        }
    }

    if (claimed.any { it.conversation?.status == ConversationStatus.Working }) {
        LaunchedEffect(Unit) {
            while (true) {
                delay(TickMillis)
                now = kotlin.time.Clock.System.now()
            }
        }
    }

    fun toggle(lane: Lane) {
        collapsed = if (lane in collapsed) collapsed - lane else collapsed + lane
        platform.savePreference(CollapsedPreference, savedLanes(collapsed))
    }

    val projects = state.projects.values.sortedBy { it.name }
    val nodes = state.nodes.values.sortedBy { it.name }
    val agents = nodes.flatMap { node -> node.agents.map { it.agent } }.distinct().sorted()
    val chosenProject = projects.firstOrNull { it.id == filter.projectId }
    val chosenNode = nodes.firstOrNull { it.id == filter.nodeId }

    Column(Modifier.fillMaxSize()) {
        Bar {
            BarTitle("Tasks")
            Box(Modifier.weight(1f))

            Box {
                MyButton(
                    { projectMenu = true },
                    text = chosenProject?.name ?: "All projects",
                    description = "Project: ${chosenProject?.name ?: "All projects"}",
                    variant = ButtonVariant.Outline,
                    size = ButtonSize.Sm,
                    muted = chosenProject == null,
                    trailingIcon = MyIcon.ChevronDown,
                )

                if (projectMenu) {
                    MenuPopup(onDismiss = { projectMenu = false }) {
                        MenuLabel("Project")
                        MenuItem("All projects", onClick = { filter = filter.copy(projectId = null); projectMenu = false }, checked = filter.projectId == null)

                        for (project in projects) {
                            MenuItem(project.name, onClick = { filter = filter.copy(projectId = project.id); projectMenu = false }, checked = project.id == filter.projectId)
                        }
                    }
                }
            }

            MyButton({ adding = true }, text = "Add a task", icon = MyIcon.Plus, variant = ButtonVariant.Outline, modifier = Modifier.padding(start = Tokens.Space.S2))
        }

        FlowRow(
            Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
            verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
            itemVerticalAlignment = Alignment.CenterVertically,
        ) {
            TextInput(filter.text, { filter = filter.copy(text = it) }, label = "Filter tasks", placeholder = "Filter tasks", showLabel = false, modifier = Modifier.width(Tokens.Size.FormColumn))

            Box {
                MyButton(
                    { nodeMenu = true },
                    text = chosenNode?.name ?: "All nodes",
                    description = "Node: ${chosenNode?.name ?: "All nodes"}",
                    variant = ButtonVariant.Outline,
                    size = ButtonSize.Sm,
                    muted = chosenNode == null,
                    trailingIcon = MyIcon.ChevronDown,
                )

                if (nodeMenu) {
                    MenuPopup(onDismiss = { nodeMenu = false }) {
                        MenuLabel("Node")
                        MenuItem("All nodes", onClick = { filter = filter.copy(nodeId = null); nodeMenu = false }, checked = filter.nodeId == null)

                        for (node in nodes) {
                            MenuItem(node.name, onClick = { filter = filter.copy(nodeId = node.id); nodeMenu = false }, checked = node.id == filter.nodeId, leading = nodeIcon(node))
                        }
                    }
                }
            }

            Box {
                MyButton(
                    { agentMenu = true },
                    text = filter.agent?.let(::agentName) ?: "All agents",
                    description = "Agent: ${filter.agent?.let(::agentName) ?: "All agents"}",
                    variant = ButtonVariant.Outline,
                    size = ButtonSize.Sm,
                    muted = filter.agent == null,
                    trailingIcon = MyIcon.ChevronDown,
                )

                if (agentMenu) {
                    MenuPopup(onDismiss = { agentMenu = false }) {
                        MenuLabel("Agent")
                        MenuItem("All agents", onClick = { filter = filter.copy(agent = null); agentMenu = false }, checked = filter.agent == null)

                        for (agent in agents) {
                            MenuItem(agentName(agent), onClick = { filter = filter.copy(agent = agent); agentMenu = false }, checked = agent == filter.agent, leading = agentIcon(agent))
                        }
                    }
                }
            }

            MyButton(
                { filter = filter.copy(showDeclined = !filter.showDeclined) },
                text = if (filter.showDeclined) "Hide declined" else "Show declined",
                variant = ButtonVariant.Outline,
                size = ButtonSize.Sm,
                muted = !filter.showDeclined,
            )
        }

        Hairline()

        BoxWithConstraints(Modifier.weight(1f).fillMaxWidth()) {
            val open = Lane.entries.filter { it !in collapsed }
            val gaps = Tokens.Space.S2 * (Lane.entries.size - 1) + Tokens.Space.S3 * 2
            val share = (maxWidth - gaps - CollapsedLaneWidth * (Lane.entries.size - open.size)) / open.size.coerceAtLeast(1)
            val laneWidth = maxOf(LaneMinWidth, share)

            Row(
                Modifier.fillMaxSize().horizontalScroll(rememberScrollState()).padding(Tokens.Space.S3),
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
            ) {
                for (lane in Lane.entries) {
                    val cards = lanes.getValue(lane)

                    if (lane in collapsed) {
                        CollapsedLane(lane, cards.size, onExpand = { toggle(lane) })
                    } else {
                        LaneColumn(lane, cards, laneWidth, onCollapse = { toggle(lane) }) { card ->
                            TaskCardView(card, store, state, now, onOpen, onAnswer = { answering = it })
                        }
                    }
                }
            }
        }
    }

    if (adding) {
        AddTaskDialog(store, state, onDismiss = { adding = false })
    }

    answering?.let { task ->
        AnswerTaskDialog(store, task, onDismiss = { answering = null })
    }
}

@Composable
private fun LaneColumn(lane: Lane, cards: List<BoardCard>, width: androidx.compose.ui.unit.Dp, onCollapse: () -> Unit, card: @Composable (BoardCard) -> Unit) {
    val colors = MyTheme.colors

    Column(
        Modifier.width(width).fillMaxHeight().background(colors.bgPanel, BlockShape).padding(Tokens.Space.S2),
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
    ) {
        Row(Modifier.fillMaxWidth().heightIn(min = rowHeight()), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalAlignment = Alignment.CenterVertically) {
            Icon(laneIcon(lane), laneColour(lane), Tokens.Size.IconSm)
            Label(lane.title, Modifier.semantics { heading() }, Tokens.Type.UiStrong, maxLines = 1)
            Label(cards.size.toString(), color = colors.textFaint)
            Box(Modifier.weight(1f))
            MyButton(onCollapse, icon = MyIcon.ChevronDown, description = "Collapse ${lane.title}", size = ButtonSize.Sm, muted = true)
        }

        Column(Modifier.weight(1f).verticalScrollWithBar(), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            for (item in cards) {
                card(item)
            }
        }
    }
}

@Composable
private fun CollapsedLane(lane: Lane, count: Int, onExpand: () -> Unit) {
    val colors = MyTheme.colors

    Column(
        Modifier
            .width(CollapsedLaneWidth)
            .fillMaxHeight()
            .pressable(onExpand, description = "Expand ${lane.title}, $count", shape = BlockShape, base = colors.bgPanel)
            .padding(vertical = Tokens.Space.S3),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
    ) {
        Icon(laneIcon(lane), laneColour(lane), Tokens.Size.IconSm)
        Label(count.toString(), color = colors.textFaint)
        Label(lane.title, Modifier.vertical(), Tokens.Type.UiStrong, colors.textMuted, maxLines = 1)
    }
}

@Composable
private fun TaskCardView(card: BoardCard, store: Store, state: AppState, now: kotlin.time.Instant, onOpen: (String) -> Unit, onAnswer: (Task) -> Unit) {
    val colors = MyTheme.colors
    val conversation = card.conversation
    val events = conversation?.let { state.events[it.id] }
    val permission = remember(events) { events?.let { pendingPermission(it) } }
    var explained by remember { mutableStateOf(false) }
    var startMenu by remember { mutableStateOf(false) }

    Column(
        Modifier.fillMaxWidth().background(colors.bgRaised, BlockShape).padding(Tokens.Space.S2),
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5),
    ) {
        if (conversation != null) {
            Label(card.title, Modifier.fillMaxWidth().pressable({ onOpen(conversation.id) }, description = "Open ${card.title}", shape = ControlShape), Tokens.Type.UiStrong)
        } else {
            Label(card.title, style = Tokens.Type.UiStrong)
        }

        Label(card.details, style = Tokens.Type.Meta, color = colors.textMuted)

        if (conversation != null && conversation.status == ConversationStatus.Failed) {
            StatusMark("Failed", Tone.Failed, MyIcon.Failed)
        }

        if (conversation != null && conversation.status == ConversationStatus.Working) {
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                Label(elapsed(conversation.updatedAt, now), Modifier.weight(1f).semantics { contentDescription = "Running for ${elapsed(conversation.updatedAt, now)}" }, Tokens.Type.Meta, colors.textMuted)
                MyButton({ store.act { store.api.cancel(conversation.id) } }, text = "Stop", icon = MyIcon.Stop, description = "Stop ${card.title}", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
            }
        }

        if (conversation != null && conversation.status == ConversationStatus.Input) {
            if (permission == null) {
                MyButton({ onOpen(conversation.id) }, text = "Answer", description = "Answer ${card.title}", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
            } else {
                Label(permission.title, style = Tokens.Type.Meta, color = colors.text)

                FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5)) {
                    for (option in permission.options) {
                        MyButton(
                            { store.act { store.api.answerPermission(conversation.id, permission.requestId, option.id) } },
                            text = option.name,
                            description = "${option.name}: ${permission.title}",
                            variant = ButtonVariant.Outline,
                            size = ButtonSize.Sm,
                        )
                    }
                }
            }
        }

        card.task?.let { task ->
            when (task.status) {
                TaskStatuses.Question -> MyButton({ onAnswer(task) }, text = "Answer", description = "Answer ${task.title}", variant = ButtonVariant.Outline, size = ButtonSize.Sm)

                TaskStatuses.Proposed -> Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5)) {
                    MyButton({ store.act { store.approveTask(task) } }, text = "Approve", description = "Approve ${task.title}", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
                    MyButton({ store.act { store.declineTask(task) } }, text = "Decline", description = "Decline ${task.title}", size = ButtonSize.Sm, muted = true)
                }

                TaskStatuses.Open -> {
                    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5)) {
                        Box {
                            MyButton(
                                {
                                    when {
                                        card.waitingOn.isNotEmpty() -> explained = true
                                        card.sendTargets.isEmpty() -> store.act { store.startTask(task) }
                                        else -> startMenu = true
                                    }
                                },
                                text = "Start",
                                description = "Start ${task.title}",
                                variant = ButtonVariant.Outline,
                                size = ButtonSize.Sm,
                                muted = card.waitingOn.isNotEmpty(),
                                trailingIcon = if (card.sendTargets.isEmpty() || card.waitingOn.isNotEmpty()) null else MyIcon.ChevronDown,
                            )

                            if (startMenu) {
                                MenuPopup(onDismiss = { startMenu = false }) {
                                    MenuItem("New conversation", onClick = { startMenu = false; store.act { store.startTask(task) } })

                                    for (target in card.sendTargets) {
                                        MenuItem("${if (target.acceptsAMessageNow) "Send to" else "Queue for"} ${target.title}", onClick = { startMenu = false; store.act { store.startTask(task, target.id) } })
                                    }
                                }
                            }
                        }
                        MyButton({ store.act { store.deleteTask(task) } }, text = "Delete", description = "Delete ${task.title}", size = ButtonSize.Sm, muted = true)
                    }

                    if (explained && card.waitingOn.isNotEmpty()) {
                        Label("Finish ${card.waitingOn.joinToString(", ") { it.title }} before starting this task.", style = Tokens.Type.MetaStrong, color = colors.statusFailed)
                    }
                }

                TaskStatuses.Claimed -> if (conversation != null && conversation.status in finishedStatuses) {
                    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                        if (conversation.status != ConversationStatus.Failed) {
                            StatusMark("Ready to review", Tone.Attention, MyIcon.Review, Modifier.weight(1f))
                        } else {
                            Box(Modifier.weight(1f))
                        }

                        MyButton({ store.act { store.finishTask(task) } }, text = "Mark done", description = "Mark ${task.title} done", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
                    }
                }

                TaskStatuses.Declined -> StatusMark("Declined", Tone.Muted, MyIcon.Close)

                TaskStatuses.Done -> task.answer?.let { Label("Answered: $it", style = Tokens.Type.Meta, color = colors.text) }
            }
        }
    }
}

@Composable
private fun AnswerTaskDialog(store: Store, task: Task, onDismiss: () -> Unit) {
    Dialog(onDismiss) {
        Label("Answer a question", Modifier.semantics { heading() }, Tokens.Type.Title)
        Label(task.title, style = Tokens.Type.UiStrong)

        AnswerForm(task, send = { text ->
            store.answerTask(task, text)
            onDismiss()
        }, onCancel = onDismiss)
    }
}

@Composable
internal fun AnswerForm(task: Task, send: suspend (String) -> Unit, spacing: Dp = Tokens.Space.S4, onCancel: (() -> Unit)? = null) {
    val scope = rememberCoroutineScope()
    var answer by remember { mutableStateOf("") }
    var answerError by remember { mutableStateOf<String?>(null) }
    var failure by remember { mutableStateOf<String?>(null) }
    var running by remember { mutableStateOf(false) }

    fun submit(text: String) {
        if (running) {
            return
        }

        answerError = if (text.isBlank()) "Enter an answer to send." else null

        if (answerError != null) {
            return
        }

        failure = null
        running = true

        scope.launch {
            failure = attempt { send(text.trim()) }
            running = false
        }
    }

    Column(verticalArrangement = Arrangement.spacedBy(spacing)) {
        if (task.options.isNotEmpty()) {
            FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5)) {
                for (option in task.options) {
                    MyButton({ submit(option) }, text = option, description = "Answer ${task.title}: $option", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
                }
            }
        }

        TextInput(answer, { answer = it }, label = "Answer", placeholder = "Your answer", error = answerError, onSubmit = { submit(answer) })

        failure?.let { ErrorBanner(it, onDismiss = { failure = null }) }

        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End)) {
            onCancel?.let { MyButton(it, text = "Cancel", muted = true) }
            MyButton({ submit(answer) }, text = "Send answer", variant = ButtonVariant.Outline)
        }
    }
}

@Composable
private fun AddTaskDialog(store: Store, state: AppState, onDismiss: () -> Unit) {
    val scope = rememberCoroutineScope()
    val place = remember { Place() }
    var title by remember { mutableStateOf("") }
    var prompt by remember { mutableStateOf("") }
    var dependsOn by remember { mutableStateOf(emptySet<String>()) }
    val dependencies = state.tasks.values.filter { it.status == TaskStatuses.Open || it.status == TaskStatuses.Claimed }.sortedBy { it.createdAt }
    var titleError by remember { mutableStateOf<String?>(null) }
    var cwdError by remember { mutableStateOf<String?>(null) }
    var failure by remember { mutableStateOf<String?>(null) }
    var running by remember { mutableStateOf(false) }

    fun submit() {
        val target = place.target(state)

        if (running || target == null) {
            return
        }

        titleError = if (title.isBlank()) "Enter a title for this task." else null
        cwdError = if (target.project == null && place.cwd.isBlank()) "Enter the folder the agent should work in, for example /home/you/project." else null

        if (titleError != null || cwdError != null) {
            return
        }

        val request = CreateTaskRequest(
            nodeId = target.node.id,
            agent = target.agent,
            title = title.trim(),
            cwd = if (target.project == null) place.cwd.trim() else null,
            projectId = target.project?.id,
            branch = place.branch.trim().ifEmpty { null },
            prompt = prompt.trim().ifEmpty { null },
            dependsOn = dependencies.map { it.id }.filter { it in dependsOn },
        )

        failure = null
        running = true

        scope.launch {
            failure = attempt {
                store.createTask(request)
                onDismiss()
            }
            running = false
        }
    }

    Dialog(onDismiss) {
        Label("Add a task", Modifier.semantics { heading() }, Tokens.Type.Title)

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
            TextInput(title, { title = it }, label = "Title", placeholder = "Rebuild install.md for the new flags", error = titleError, onSubmit = ::submit)
            TextInput(prompt, { prompt = it }, label = "Prompt (optional)", placeholder = "Defaults to the title")
            PlacePicker(store, state, place, withBranch = true, cwdError = cwdError)

            if (dependencies.isNotEmpty()) {
                Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                    Label("Waits on (optional)", style = Tokens.Type.MetaStrong, color = MyTheme.colors.textMuted)

                    FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5)) {
                        for (dependency in dependencies) {
                            val chosen = dependency.id in dependsOn

                            MyButton(
                                { dependsOn = if (chosen) dependsOn - dependency.id else dependsOn + dependency.id },
                                text = dependency.title,
                                description = "Waits on ${dependency.title}",
                                variant = ButtonVariant.Outline,
                                size = ButtonSize.Sm,
                                muted = !chosen,
                                selected = chosen,
                            )
                        }
                    }
                }
            }
        }

        failure?.let { ErrorBanner(it, onDismiss = { failure = null }) }

        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End)) {
            MyButton(onDismiss, text = "Cancel", muted = true)
            MyButton(::submit, text = "Add task", variant = ButtonVariant.Outline)
        }
    }
}
