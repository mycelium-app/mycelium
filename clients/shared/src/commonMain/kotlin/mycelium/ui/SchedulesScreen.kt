package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.unit.dp
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import kotlinx.coroutines.launch
import mycelium.api.CreateScheduleRequest
import mycelium.api.EachRun
import mycelium.api.PatchScheduleRequest
import mycelium.api.Schedule
import mycelium.api.ScheduleRun
import mycelium.state.AppState
import mycelium.state.scheduleInGroup
import mycelium.state.Store
import mycelium.state.Cadence
import mycelium.state.WhenKind
import mycelium.state.Weekdays
import mycelium.state.agentName
import mycelium.state.cadenceOf
import mycelium.state.cronFor
import mycelium.state.describeCron
import mycelium.state.eachRunName
import mycelium.state.projectName
import mycelium.state.statusSectionName
import mycelium.state.until

private const val SkippedOutcome = "skipped"
private const val HeldOutcome = "held"

@Composable
fun SchedulesScreen(store: Store, state: AppState) {
    val colors = MyTheme.colors
    val stamp = timestamper()
    var creating by remember { mutableStateOf(false) }

    LaunchedEffect(state.conversations.size) {
        store.act {
            store.refreshProjects()
            store.refreshSchedules()
        }
    }

    val group = LocalGroupFilter.current
    val schedules = state.schedules.values.filter { scheduleInGroup(it, state, group) }.sortedBy { it.createdAt }
    val shownIds = schedules.map { it.id }.toSet()
    val runs = state.scheduleRuns.filter { it.scheduleId in shownIds }

    Column(Modifier.fillMaxSize()) {
        Bar {
            BarTitle("Schedules")
            Box(Modifier.weight(1f))
            MyButton({ creating = true }, text = "New schedule", icon = MyIcon.Plus, variant = ButtonVariant.Outline)
        }

        Column(Modifier.weight(1f).fillMaxWidth().verticalScrollWithBar().padding(Tokens.Space.S4), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4)) {
            if (schedules.isNotEmpty()) {
                Table(
                    listOf(
                        Col("Prompt", 100.dp, weight = 1f),
                        Col("Project", 100.dp, max = 240.dp, priority = 1),
                        Col("When", 100.dp, max = 240.dp, priority = 1),
                        Col("Each run", 88.dp, priority = 1),
                        Col("Next", 72.dp, max = 120.dp),
                        ActionColumn,
                    ),
                    schedules.size,
                    { row, column ->
                        val schedule = schedules[row]

                        when (column) {
                            0 -> TableCell.Text(schedule.prompt)
                            1 -> TableCell.Text(state.assistants[schedule.assistantId]?.name ?: state.projects[schedule.projectId]?.name ?: projectName(schedule.cwd), color = colors.textMuted)
                            2 -> TableCell.Slot(listOf(describeCron(schedule.cron), schedule.timeZone).maxBy { it.length }) {
                                Column {
                                    Label(describeCron(schedule.cron), style = Tokens.Type.Code, maxLines = 1)
                                    Label(schedule.timeZone, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
                                }
                            }

                            3 -> TableCell.Text(eachRunName(schedule.eachRun), color = colors.textMuted)
                            4 -> TableCell.Text(if (schedule.proposed) "Waiting for you" else if (schedule.paused) "Paused" else until(schedule.nextRunAt), color = if (schedule.paused) colors.textFaint else colors.text)
                            else -> TableCell.Slot { ScheduleActions(schedule, store, state) }
                        }
                    },
                )
            }

            if (runs.isNotEmpty()) {
                Table(
                    listOf(
                        Col("Last runs", 72.dp),
                        Col("Conversation", 96.dp, weight = 1f),
                        Col("Result", 88.dp, max = 240.dp),
                    ),
                    runs.size,
                    { row, column ->
                        val run = runs[row]
                        val conversation = state.conversations[run.conversationId]

                        when (column) {
                            0 -> TableCell.Text(stamp(run.ranAt, true), color = colors.textMuted)
                            1 -> TableCell.Text(conversation?.title ?: state.schedules[run.scheduleId]?.prompt.orEmpty())
                            else -> TableCell.Slot(if (run.outcome == SkippedOutcome) "Skipped: ${run.reason.orEmpty()}" else if (run.outcome == HeldOutcome) "Held: ${run.reason.orEmpty()}" else conversation?.let { statusSectionName(it.status) }.orEmpty(), Tokens.Size.IconSm + Tokens.Space.S1_5) {
                                Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalAlignment = Alignment.CenterVertically) {
                                    if (run.outcome == SkippedOutcome) {
                                        StatusMark("Skipped: ${run.reason.orEmpty()}", Tone.Attention, MyIcon.Alert)
                                    } else if (run.outcome == HeldOutcome) {
                                        StatusMark("Held: ${run.reason.orEmpty()}", Tone.Attention, MyIcon.Alert)
                                    } else if (conversation != null) {
                                        StatusGlyph(conversation.status)
                                        Label(statusSectionName(conversation.status), color = colors.text, maxLines = 1)
                                    }
                                }
                            }
                        }
                    },
                    headerCell = { column ->
                        if (column == 0) {
                            TableCell.Slot("Last runs") { Label("Last runs", Modifier.semantics { heading() }, Tokens.Type.MetaStrong, colors.textMuted) }
                        } else {
                            TableCell.Text(if (column == 1) "Conversation" else "Result", Tokens.Type.MetaStrong)
                        }
                    },
                )
            }

            if (schedules.isEmpty()) {
                Label("No schedule is set up. Add one to start a conversation on its own.", color = colors.textMuted)
            }
        }
    }

    if (creating) {
        ScheduleDialog(store, state, onDismiss = { creating = false })
    }
}

@Composable
private fun ScheduleActions(schedule: Schedule, store: Store, state: AppState) {
    val colors = MyTheme.colors
    var reviewing by remember(schedule.id) { mutableStateOf(false) }
    var editing by remember(schedule.id) { mutableStateOf(false) }
    var confirmingDelete by remember(schedule.id) { mutableStateOf(false) }

    RowMenu(schedule.prompt) { close ->
        if (schedule.proposed) {
            MenuItem("Review", onClick = { close(); reviewing = true }, leading = MyIcon.Review)
            MenuItem("Decline", onClick = { close(); store.act { store.declineSchedule(schedule) } }, leading = MyIcon.Close)
        } else {
            MenuItem("Run now", onClick = { close(); store.act { store.runSchedule(schedule) } }, leading = MyIcon.Refresh)
            MenuItem("Edit", onClick = { close(); editing = true }, leading = MyIcon.Pencil)
            MenuItem(if (schedule.paused) "Resume" else "Pause", onClick = { close(); store.act { store.pauseSchedule(schedule, !schedule.paused) } }, leading = MyIcon.Clock)
            MenuItem("Delete", onClick = { close(); confirmingDelete = true }, leading = MyIcon.Close)
        }
    }

    if (reviewing) {
        ProposedScheduleDialog(store, schedule, state, onDismiss = { reviewing = false })
    }

    if (editing) {
        ScheduleDialog(store, state, schedule, onDismiss = { editing = false })
    }

    if (confirmingDelete) {
        ConfirmDialog("Delete this schedule?", "It stops running and its settings are removed. Conversations it already started are kept.", "Delete schedule", onDismiss = { confirmingDelete = false }, onConfirm = { confirmingDelete = false; store.act { store.deleteSchedule(schedule) } })
    }
}

@Composable
private fun ScheduleDialog(store: Store, state: AppState, editing: Schedule? = null, onDismiss: () -> Unit) {
    val scope = rememberCoroutineScope()
    val place = remember { Place() }
    val timeZone = editing?.timeZone ?: LocalPlatform.current?.timeZone() ?: "UTC"
    val start = remember { editing?.let { cadenceOf(it.cron) } ?: Cadence() }
    var prompt by remember { mutableStateOf(editing?.prompt.orEmpty()) }
    var kind by remember { mutableStateOf(start.kind) }
    var every by remember { mutableStateOf(start.every) }
    var hours by remember { mutableStateOf(start.hours) }
    var at by remember { mutableStateOf(start.at) }
    var weekday by remember { mutableStateOf(start.weekday) }
    var custom by remember { mutableStateOf(start.custom) }
    var eachRun by remember { mutableStateOf(editing?.eachRun ?: EachRun.New) }
    var promptError by remember { mutableStateOf<String?>(null) }
    var whenError by remember { mutableStateOf<String?>(null) }
    var cwdError by remember { mutableStateOf<String?>(null) }
    var failure by remember { mutableStateOf<String?>(null) }
    var running by remember { mutableStateOf(false) }

    fun finish(action: suspend () -> Unit) {
        failure = null
        running = true

        scope.launch {
            failure = attempt {
                action()
                onDismiss()
            }
            running = false
        }
    }

    fun submit() {
        val target = place.target(state)

        if (running || (editing == null && target == null)) {
            return
        }

        val cron = cronFor(kind, every.trim().toIntOrNull() ?: 0, hours, at, weekday, custom)

        promptError = if (prompt.isBlank()) "Enter the prompt to run on this schedule." else null
        cwdError = if (editing == null && target?.project == null && place.cwd.isBlank()) "Enter the folder the agent should work in, for example /home/you/project." else null
        whenError = if (cron == null) {
            when (kind) {
                WhenKind.Every -> "Enter how often, as a whole number above zero."
                WhenKind.Cron -> "Enter the schedule as five cron fields, for example 0 6 * * 1."
                else -> "Enter the time as hours and minutes, for example 06:30."
            }
        } else {
            null
        }

        if (cron == null || promptError != null || cwdError != null) {
            return
        }

        if (editing != null) {
            val changes = PatchScheduleRequest(
                prompt = prompt.trim().takeIf { it != editing.prompt },
                cron = cron.takeIf { it != editing.cron },
                eachRun = eachRun.takeIf { it != editing.eachRun },
            )

            finish { store.editSchedule(editing, changes) }

            return
        }

        if (target == null) {
            return
        }

        val request = CreateScheduleRequest(
            nodeId = target.node.id,
            agent = target.agent,
            prompt = prompt.trim(),
            cron = cron,
            eachRun = eachRun,
            cwd = if (target.project == null) place.cwd.trim() else null,
            projectId = target.project?.id,
            timeZone = timeZone,
        )

        finish { store.createSchedule(request) }
    }

    Dialog(onDismiss) {
        Label(if (editing == null) "New schedule" else "Edit schedule", Modifier.semantics { heading() }, Tokens.Type.Title)

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
            TextInput(prompt, { prompt = it }, label = "Prompt", placeholder = "Update and test dependencies", error = promptError)

            if (editing == null) {
                PlacePicker(store, state, place, withBranch = false, cwdError = cwdError)
            } else {
                Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                    Label("Runs in", style = Tokens.Type.MetaStrong, color = MyTheme.colors.textMuted)
                    Label(state.projects[editing.projectId]?.name ?: editing.cwd)
                    Label("${agentName(editing.agent)} on ${state.nodes[editing.nodeId]?.name.orEmpty()}", color = MyTheme.colors.textMuted)
                }
            }

            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                Label("When", style = Tokens.Type.MetaStrong, color = MyTheme.colors.textMuted)
                Segmented(WhenKind.entries.map { it.label }, kind.label, { picked -> kind = WhenKind.entries.first { it.label == picked } }, groupLabel = "When")
            }

            when (kind) {
                WhenKind.Every -> {
                    TextInput(every, { every = it }, label = "Every", mono = true, error = whenError)
                    Segmented(listOf("Hours", "Minutes"), if (hours) "Hours" else "Minutes", { hours = it == "Hours" }, groupLabel = "Unit")
                }

                WhenKind.Daily -> TextInput(at, { at = it }, label = "At ($timeZone)", placeholder = "06:00", mono = true, error = whenError)

                WhenKind.Weekly -> {
                    Segmented(Weekdays, weekday, { weekday = it }, groupLabel = "Day")
                    TextInput(at, { at = it }, label = "At ($timeZone)", placeholder = "06:00", mono = true, error = whenError)
                }

                WhenKind.Cron -> TextInput(custom, { custom = it }, label = "Cron expression ($timeZone)", placeholder = "0 6 * * 1", mono = true, error = whenError)
            }

            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                Label("Each run", style = Tokens.Type.MetaStrong, color = MyTheme.colors.textMuted)
                Segmented(
                    listOf(eachRunName(EachRun.New), eachRunName(EachRun.Same)),
                    eachRunName(eachRun),
                    { picked -> eachRun = if (picked == eachRunName(EachRun.Same)) EachRun.Same else EachRun.New },
                    groupLabel = "Each run",
                )
            }
        }

        failure?.let { ErrorBanner(it, onDismiss = { failure = null }) }

        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End)) {
            MyButton(onDismiss, text = "Cancel", muted = true)
            MyButton(::submit, text = if (editing == null) "Add schedule" else "Save", variant = ButtonVariant.Outline)
        }
    }
}
