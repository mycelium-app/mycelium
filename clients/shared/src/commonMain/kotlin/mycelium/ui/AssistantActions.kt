package mycelium.ui

import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import mycelium.api.ActionInput
import mycelium.api.ActionProposal
import mycelium.api.Assistant
import mycelium.api.AssistantAction
import mycelium.api.ModelChoice
import mycelium.api.ModelProvider
import mycelium.api.SaveActionRequest
import mycelium.state.AppState
import mycelium.state.Choice
import mycelium.state.Store
import mycelium.state.TodayDefault
import mycelium.state.askedFor
import mycelium.state.describeCron
import mycelium.state.expandAction
import mycelium.state.missingInput
import mycelium.state.until
import kotlin.time.Clock

private val ChipShape = RoundedCornerShape(percent = 50)
private const val NoModel = ""
private const val AnyNode = ""
private val InputKinds = listOf(Choice("text", "Text"), Choice("date", "Date"), Choice("choice", "Choice"))

private fun today(): String = Clock.System.now().toString().take(10)

@Composable
internal fun ActionInputField(input: ActionInput, value: String, onChange: (String) -> Unit, error: String? = null) {
    val label = input.name.replaceFirstChar { it.uppercase() }

    when (input.kind) {
        "choice" -> Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            Label(label, style = Tokens.Type.MetaStrong, color = MyTheme.colors.textMuted)
            OptionMenu(label, input.options.map { Choice(it, it) }, value.ifEmpty { input.default }, onChange, title = label)
            error?.let { Label(it, style = Tokens.Type.MetaStrong, color = MyTheme.colors.statusFailed) }
        }

        "date" -> TextInput(value, onChange, label = label, placeholder = today(), mono = true, error = error)
        else -> TextInput(value, onChange, label = label, error = error)
    }
}

class ActionMenuState {
    var open by mutableStateOf(false)
    var asking by mutableStateOf<AssistantAction?>(null)
}

class AssistantActionControls(
    val assistant: Assistant,
    val actions: List<AssistantAction>,
    val menu: ActionMenuState,
    private val run: (AssistantAction, Map<String, String>) -> Unit,
    val onManage: () -> Unit,
) {
    fun choose(action: AssistantAction) {
        if (action.inputs.isEmpty()) {
            run(action, emptyMap())
        } else {
            menu.asking = action
        }
    }

    fun answer(action: AssistantAction, inputs: Map<String, String>) {
        menu.asking = null
        run(action, inputs)
    }
}

@Composable
fun rememberAssistantActionControls(assistant: Assistant, actions: List<AssistantAction>, onManage: () -> Unit, run: (AssistantAction, Map<String, String>) -> Unit): AssistantActionControls {
    val menu = remember(assistant.id) { ActionMenuState() }

    return AssistantActionControls(assistant, actions, menu, run, onManage)
}

@Composable
internal fun ActionChips(controls: AssistantActionControls) {
    val pinned = controls.actions.filter { it.pinned }

    if (pinned.isEmpty()) {
        return
    }

    FlowRow(Modifier.fillMaxWidth().padding(bottom = Tokens.Space.S1), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
        for (action in pinned) {
            Row(
                Modifier
                    .heightIn(min = touchMin(Tokens.Size.ControlSm))
                    .border(Tokens.Size.Hairline, MyTheme.colors.borderControl, ChipShape)
                    .pressable({ controls.choose(action) }, description = "Run ${action.name}", shape = ChipShape)
                    .padding(horizontal = Tokens.Space.S2),
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Icon(MyIcon.Zap, MyTheme.colors.textMuted, Tokens.Size.IconSm)
                Label(action.name, style = Tokens.Type.Meta, maxLines = 1)
            }
        }
    }
}

@Composable
internal fun ActionsButton(controls: AssistantActionControls) {
    val menu = controls.menu
    var search by remember(menu.open) { mutableStateOf("") }
    val shown = controls.actions.filter { search.isBlank() || it.name.contains(search, ignoreCase = true) }

    Box {
        MyButton({ menu.open = true }, icon = MyIcon.Zap, description = "Actions", muted = true, selected = menu.open, tooltip = "Actions, Ctrl+Shift+A")

        if (menu.open) {
            MenuPopup(onDismiss = { menu.open = false }) {
                TextInput(search, { search = it }, label = "Search actions", showLabel = false, placeholder = "Search actions", leadingIcon = MyIcon.Search, modifier = Modifier.padding(Tokens.Space.S1))
                MenuLabel("${controls.assistant.name} actions")

                for (action in shown) {
                    MenuItem(
                        action.name,
                        onClick = { menu.open = false; controls.choose(action) },
                        hint = action.inputs.takeIf { it.isNotEmpty() }?.let { "Asks for ${it.joinToString(", ") { input -> input.name }}" },
                        leading = MyIcon.Zap,
                        trailing = if (action.pinned) ({ Icon(MyIcon.Pin, MyTheme.colors.textFaint, Tokens.Size.IconSm) }) else null,
                    )
                }

                if (shown.isEmpty()) {
                    MenuLabel("No matching actions")
                }

                Hairline()
                MenuItem("Manage actions", onClick = { menu.open = false; controls.onManage() }, leading = MyIcon.Sliders)
            }
        }
    }

    menu.asking?.let { action ->
        RunActionDialog(action, onDismiss = { menu.asking = null }, onRun = { inputs -> controls.answer(action, inputs) })
    }
}

@Composable
internal fun RunActionDialog(action: AssistantAction, onDismiss: () -> Unit, onRun: (Map<String, String>) -> Unit) {
    var values by remember(action.id) { mutableStateOf(emptyMap<String, String>()) }
    var missing by remember(action.id) { mutableStateOf<String?>(null) }
    val now = today()

    fun run() {
        val lacking = missingInput(action, values, now)

        if (lacking != null) {
            missing = lacking.name

            return
        }

        onRun(values)
    }

    Dialog(onDismiss) {
        Label(action.name, Modifier.semantics { heading() }, Tokens.Type.Title)

        for (input in action.inputs) {
            ActionInputField(input, values[input.name].orEmpty(), { values = values + (input.name to it); missing = null }, error = if (missing == input.name) "Enter a value for ${input.name}." else null)
        }

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            Label("Sends", style = Tokens.Type.MetaStrong, color = MyTheme.colors.textMuted)
            Label(expandAction(action, values, now), color = MyTheme.colors.textMuted)
        }

        FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            MyButton(onDismiss, text = "Cancel", muted = true)
            MyButton(::run, text = "Run", variant = ButtonVariant.Outline)
        }
    }
}

private fun modelChoices(providers: List<ModelProvider>): List<Choice> =
    listOf(Choice("agent:claude", "Claude Code"), Choice("agent:codex", "Codex")) + providers.flatMap { provider -> provider.models.map { Choice("provider:${provider.id}:$it", "${provider.name} · $it") } }

private fun modelKey(model: ModelChoice?): String = when {
    model == null -> NoModel
    model.agent != null -> "agent:${model.agent}"
    else -> "provider:${model.providerId}:${model.model}"
}

private fun modelFromKey(key: String): ModelChoice? = when {
    key.startsWith("agent:") -> ModelChoice(agent = key.removePrefix("agent:"))
    key.startsWith("provider:") -> key.removePrefix("provider:").split(":", limit = 2).let { ModelChoice(providerId = it[0], model = it.getOrNull(1)) }
    else -> null
}

@Composable
internal fun ActionDialog(title: String, initial: SaveActionRequest?, state: AppState, providers: List<ModelProvider>, onDismiss: () -> Unit, onSave: suspend (SaveActionRequest) -> Unit) {
    var name by remember { mutableStateOf(initial?.name.orEmpty()) }
    var prompt by remember { mutableStateOf(initial?.prompt.orEmpty()) }
    var inputs by remember { mutableStateOf(initial?.inputs.orEmpty()) }
    var model by remember { mutableStateOf(initial?.model) }
    var nodeId by remember { mutableStateOf(initial?.nodeId) }
    var pinned by remember { mutableStateOf(initial?.pinned ?: false) }
    var nameError by remember { mutableStateOf<String?>(null) }
    var promptError by remember { mutableStateOf<String?>(null) }
    var failure by remember { mutableStateOf<String?>(null) }
    val attempt = rememberAttempt { failure = it }

    fun save() {
        nameError = if (name.isBlank()) "Enter a name for the action." else null
        promptError = if (prompt.isBlank()) "Enter the prompt the action sends." else null

        if (nameError != null || promptError != null) {
            return
        }

        attempt {
            onSave(SaveActionRequest(name.trim(), prompt.trim(), inputs, model, nodeId, pinned))
            onDismiss()
        }
    }

    Dialog(onDismiss) {
        Label(title, Modifier.semantics { heading() }, Tokens.Type.Title)

        Column(Modifier.verticalScrollWithBar().heightIn(max = 520.dp), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
            TextInput(name, { name = it; nameError = null }, label = "Name", placeholder = "Stand-up notes", error = nameError)
            TextInput(prompt, { prompt = it; promptError = null }, label = "Prompt", placeholder = "Write the stand-up notes for {meeting}", error = promptError)

            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                Label("Inputs", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
                Label("Asked for before the action runs. Use {name} in the prompt.", style = Tokens.Type.Meta, color = MyTheme.colors.textMuted)

                for ((index, input) in inputs.withIndex()) {
                    InputRow(input, { changed -> inputs = inputs.mapIndexed { position, held -> if (position == index) changed else held } }, { inputs = inputs.filterIndexed { position, _ -> position != index } })
                }

                Row {
                    MyButton({ inputs = inputs + ActionInput(name = "input${inputs.size + 1}") }, text = "Add input", icon = MyIcon.Plus, variant = ButtonVariant.Outline)
                }
            }

            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                Label("Model", style = Tokens.Type.MetaStrong, color = MyTheme.colors.textMuted)
                OptionMenu("Model", listOf(Choice(NoModel, "Assistant's models")) + modelChoices(providers), modelKey(model), { picked -> model = modelFromKey(picked) }, title = "Model")
            }

            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                Label("Node", style = Tokens.Type.MetaStrong, color = MyTheme.colors.textMuted)
                OptionMenu("Node", listOf(Choice(AnyNode, "Assistant's nodes")) + state.nodes.values.sortedBy { it.name.lowercase() }.map { Choice(it.id, it.name) }, nodeId ?: AnyNode, { nodeId = it.ifEmpty { null } }, title = "Node")
            }

            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                Checkbox(pinned, "Pin above the composer", { pinned = !pinned }, checkedState = "On", uncheckedState = "Off")
                Label("Pin above the composer")
            }
        }

        failure?.let { ErrorBanner(it, onDismiss = { failure = null }) }

        FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            MyButton(onDismiss, text = "Cancel", muted = true)
            MyButton(::save, text = "Save action", variant = ButtonVariant.Outline)
        }
    }
}

@Composable
private fun InputRow(input: ActionInput, onChange: (ActionInput) -> Unit, onRemove: () -> Unit) {
    Column(Modifier.fillMaxWidth().border(Tokens.Size.Hairline, MyTheme.colors.border, ControlShape).padding(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.Bottom) {
            TextInput(input.name, { onChange(input.copy(name = it)) }, label = "Input name", mono = true, modifier = Modifier.weight(1f))

            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                Label("Kind", style = Tokens.Type.MetaStrong, color = MyTheme.colors.textMuted)
                OptionMenu("Kind of ${input.name}", InputKinds, input.kind, { onChange(input.copy(kind = it, default = if (it == "date") TodayDefault else null)) }, title = "Kind")
            }

            MyButton(onRemove, icon = MyIcon.Close, description = "Remove input ${input.name}", muted = true)
        }

        if (input.kind == "choice") {
            TextInput(input.options.joinToString(", "), { text -> onChange(input.copy(options = text.split(",").map { it.trim() }.filter { it.isNotEmpty() })) }, label = "Options, separated by commas", placeholder = "Platform, Design review")
        }

        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
            Checkbox(input.required, "Required: ${input.name}", { onChange(input.copy(required = !input.required)) }, checkedState = "On", uncheckedState = "Off")
            Label("Required")
        }
    }
}

@Composable
internal fun ActionsSection(store: Store, state: AppState, assistant: Assistant) {
    val colors = MyTheme.colors
    val actions = state.assistantActions[assistant.id].orEmpty()
    val proposals = state.actionProposals.values.filter { it.assistantId == assistant.id && it.state == "proposed" }
    val schedules = state.schedules.values.filter { it.assistantId == assistant.id && !it.proposed }.sortedBy { it.createdAt }
    var editing by remember(assistant.id) { mutableStateOf<AssistantAction?>(null) }
    var creating by remember(assistant.id) { mutableStateOf(false) }
    var scheduling by remember(assistant.id) { mutableStateOf(false) }
    var editingSchedule by remember(assistant.id) { mutableStateOf<mycelium.api.Schedule?>(null) }
    var failure by remember(assistant.id) { mutableStateOf<String?>(null) }
    var providers by remember { mutableStateOf(emptyList<ModelProvider>()) }
    val attempt = rememberAttempt { failure = it }

    LaunchedEffect(assistant.id) {
        store.act { providers = store.api.modelProviders() }
        store.act { store.refreshAssistantActions(assistant.id) }
        store.act { store.refreshActionProposals(assistant.id) }
        store.act { store.refreshSchedules() }
    }

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S6)) {
        failure?.let { ErrorBanner(it, onDismiss = { failure = null }) }

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Label("Actions", Modifier.weight(1f).semantics { heading() }, Tokens.Type.UiStrong)
                MyButton({ creating = true }, text = "New action", icon = MyIcon.Plus, variant = ButtonVariant.Outline)
            }

            Label(
                if (assistant.builtinKey != null) "Built-in actions can be edited and reset, not deleted. Wellness & Learning runs them from Reflections and Exercise." else "Run from the Actions button in the thread. Pinned actions show above the composer.",
                style = Tokens.Type.Meta,
                color = colors.textMuted,
            )

            if (actions.isNotEmpty()) {
                Table(
                    listOf(Col("Action", 120.dp, weight = 1f), Col("Asks for", 88.dp, priority = 1), Col("Model", 120.dp, priority = 1), Col("Pinned", 64.dp, preferred = 64.dp), ActionColumn),
                    actions.size,
                    { row, column ->
                        val action = actions[row]

                        when (column) {
                            0 -> TableCell.Slot(action.name, if (action.builtinKey != null) 72.dp else 0.dp) {
                                Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                                    Label(action.name, maxLines = 1)

                                    if (action.builtinKey != null) {
                                        Tag("Built in")
                                    }
                                }
                            }

                            1 -> TableCell.Text(askedFor(action.inputs), color = colors.textMuted)
                            2 -> TableCell.Text(action.model?.let { it.agent?.let { agent -> if (agent == "codex") "Codex" else "Claude Code" } ?: it.model.orEmpty() } ?: "Assistant's models", color = colors.textMuted)
                            3 -> TableCell.Slot {
                                Checkbox(
                                    action.pinned,
                                    "Pinned: ${action.name}",
                                    { attempt { store.api.saveAssistantAction(assistant.id, action.id, SaveActionRequest(action.name, action.prompt, action.inputs, action.model, action.nodeId, !action.pinned)); store.refreshAssistantActions(assistant.id) } },
                                    checkedState = "Pinned",
                                    uncheckedState = "Not pinned",
                                )
                            }

                            else -> TableCell.Slot {
                                RowMenu(action.name) { close ->
                                    MenuItem("Edit", onClick = { close(); editing = action }, leading = MyIcon.Pencil)

                                    if (action.builtinKey != null) {
                                        MenuItem("Reset to default", onClick = { close(); attempt { store.api.resetAssistantAction(assistant.id, action.id); store.refreshAssistantActions(assistant.id) } }, leading = MyIcon.Refresh)
                                    } else {
                                        MenuItem("Delete", onClick = { close(); attempt { store.api.deleteAssistantAction(assistant.id, action.id); store.refreshAssistantActions(assistant.id) } }, leading = MyIcon.Close)
                                    }
                                }
                            }
                        }
                    },
                )
            }
        }

        if (proposals.isNotEmpty()) {
            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                Label("Proposed actions", Modifier.semantics { heading() }, Tokens.Type.UiStrong)

                for (proposal in proposals) {
                    ProposedActionCard(store, state, proposal, providers)
                }
            }
        }

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Label("Schedules", Modifier.weight(1f).semantics { heading() }, Tokens.Type.UiStrong)
                MyButton({ scheduling = true }, text = "New schedule", icon = MyIcon.Plus, variant = ButtonVariant.Outline)
            }

            Label("Each run happens in the ${assistant.name} thread. Schedules also lists these.", style = Tokens.Type.Meta, color = colors.textMuted)

            if (schedules.isNotEmpty()) {
                Table(
                    listOf(Col("When", 120.dp, weight = 1f), Col("Runs", 120.dp, weight = 1f), Col("Next", 72.dp, max = 120.dp), ActionColumn),
                    schedules.size,
                    { row, column ->
                        val schedule = schedules[row]

                        when (column) {
                            0 -> TableCell.Text("${describeCron(schedule.cron)} ${schedule.timeZone}")
                            1 -> TableCell.Text(actions.firstOrNull { it.id == schedule.actionId }?.name ?: schedule.prompt, color = colors.textMuted)
                            2 -> TableCell.Text(if (schedule.paused) "Paused" else until(schedule.nextRunAt), color = if (schedule.paused) colors.textFaint else colors.text)

                            else -> TableCell.Slot {
                                RowMenu(schedule.prompt) { close ->
                                    MenuItem("Run now", onClick = { close(); store.act { store.runSchedule(schedule) } }, leading = MyIcon.Refresh)
                                    MenuItem("Edit", onClick = { close(); editingSchedule = schedule }, leading = MyIcon.Pencil)
                                    MenuItem(if (schedule.paused) "Resume" else "Pause", onClick = { close(); store.act { store.pauseSchedule(schedule, !schedule.paused) } }, leading = MyIcon.Clock)
                                    MenuItem("Delete", onClick = { close(); store.act { store.deleteSchedule(schedule) } }, leading = MyIcon.Close)
                                }
                            }
                        }
                    },
                )
            }
        }
    }

    if (creating) {
        ActionDialog("New action", null, state, providers, onDismiss = { creating = false }, onSave = { request -> store.api.createAssistantAction(assistant.id, request); store.refreshAssistantActions(assistant.id) })
    }

    editing?.let { action ->
        ActionDialog(
            "Edit action",
            SaveActionRequest(action.name, action.prompt, action.inputs, action.model, action.nodeId, action.pinned),
            state,
            providers,
            onDismiss = { editing = null },
            onSave = { request -> store.api.saveAssistantAction(assistant.id, action.id, request); store.refreshAssistantActions(assistant.id) },
        )
    }

    if (scheduling) {
        ScheduleDialog(store, state, assistantId = assistant.id, onDismiss = { scheduling = false })
    }

    editingSchedule?.let { schedule ->
        ScheduleDialog(store, state, schedule, onDismiss = { editingSchedule = null })
    }
}

@Composable
internal fun ProposedActionCard(store: Store, state: AppState, proposal: ActionProposal, providers: List<ModelProvider> = emptyList()) {
    val colors = MyTheme.colors
    var editing by remember(proposal.id) { mutableStateOf(false) }
    var failure by remember(proposal.id) { mutableStateOf<String?>(null) }
    val attempt = rememberAttempt { failure = it }
    val waiting = proposal.state == "proposed"

    Column(
        Modifier.fillMaxWidth().widthIn(max = Tokens.Size.Page).border(Tokens.Size.Hairline, colors.border, ControlShape).padding(Tokens.Space.S3),
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
            Icon(MyIcon.Zap, colors.textMuted, Tokens.Size.IconSm)
            Label("Proposed an action", Modifier.weight(1f), Tokens.Type.UiStrong)
            Label(
                when (proposal.state) {
                    "approved" -> "Approved"
                    "declined" -> "Declined"
                    else -> "Waiting for approval"
                },
                style = Tokens.Type.Meta,
                color = colors.textMuted,
            )
        }

        Label(proposal.name, style = Tokens.Type.UiStrong)

        if (proposal.reason.isNotBlank()) {
            Label(proposal.reason, color = colors.textMuted)
        }

        Label(proposal.prompt, style = Tokens.Type.Code)
        Label("Asks for: ${if (proposal.inputs.isEmpty()) "nothing" else proposal.inputs.joinToString(", ") { "${it.name} (${it.kind})" }}. Pinned: no.", style = Tokens.Type.Meta, color = colors.textMuted)
        failure?.let { ErrorBanner(it, onDismiss = { failure = null }) }

        if (waiting) {
            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                MyButton({ attempt { store.api.approveActionProposal(proposal.assistantId, proposal.id); store.refreshAssistantActions(proposal.assistantId) } }, text = "Approve", variant = ButtonVariant.Outline)
                MyButton({ editing = true }, text = "Edit", variant = ButtonVariant.Outline)
                MyButton({ attempt { store.api.declineActionProposal(proposal.assistantId, proposal.id) } }, text = "Decline", muted = true)
            }
        }
    }

    if (editing) {
        ActionDialog(
            "Edit action",
            SaveActionRequest(proposal.name, proposal.prompt, proposal.inputs),
            state,
            providers,
            onDismiss = { editing = false },
            onSave = { request -> store.api.approveActionProposal(proposal.assistantId, proposal.id, request); store.refreshAssistantActions(proposal.assistantId) },
        )
    }
}

@Composable
internal fun AssistantChangeCard(store: Store, state: AppState, change: mycelium.api.AssistantChange, onOpenSettings: () -> Unit = {}) {
    val colors = MyTheme.colors
    var failure by remember(change.id) { mutableStateOf<String?>(null) }
    val attempt = rememberAttempt { failure = it }
    val creating = change.kind == "create"
    val groupName = change.groupId?.let { state.groups[it]?.name }

    Column(
        Modifier.fillMaxWidth().widthIn(max = Tokens.Size.Page).border(Tokens.Size.Hairline, colors.border, ControlShape).padding(Tokens.Space.S3),
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
            Icon(if (creating) MyIcon.Plus else MyIcon.Check, colors.textMuted, Tokens.Size.IconSm)
            Label(if (creating) "Proposed an assistant" else "Updated ${change.name}", Modifier.weight(1f), Tokens.Type.UiStrong)
            Label(
                when (change.state) {
                    "approved" -> "Created"
                    "declined" -> "Declined"
                    "applied" -> "Done"
                    "undone" -> "Undone"
                    else -> "Waiting for approval"
                },
                style = Tokens.Type.Meta,
                color = colors.textMuted,
            )
        }

        if (creating) {
            DetailLine("Name", change.name)
            groupName?.let { DetailLine("Group", it) }
            change.description?.let { DetailLine("Description", it) }
            change.instructions?.let { DetailLine("Instructions", it) }
            Label("Models, nodes, storage and secrets start at their defaults. Change them in its settings.", style = Tokens.Type.Meta, color = colors.textMuted)
        } else {
            change.description?.let { Label("Description: $it", color = colors.textMuted) }
            groupName?.let { Label("Group: $it", color = colors.textMuted) }
        }

        failure?.let { ErrorBanner(it, onDismiss = { failure = null }) }

        if (creating && change.state == "proposed") {
            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                MyButton({ attempt { store.api.approveAssistantChange(change.id); store.refreshAssistants() } }, text = "Approve", variant = ButtonVariant.Outline)
                MyButton({ attempt { store.api.declineAssistantChange(change.id) } }, text = "Decline", muted = true)
            }
        }

        if (!creating && change.state == "applied") {
            Row {
                MyButton({ attempt { store.api.undoAssistantChange(change.id); store.refreshAssistants() } }, text = "Undo", variant = ButtonVariant.Outline)
            }
        }
    }
}

@Composable
private fun DetailLine(label: String, value: String) {
    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5)) {
        Label(label, style = Tokens.Type.MetaStrong, color = MyTheme.colors.textMuted)
        Label(value)
    }
}
