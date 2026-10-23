package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import kotlinx.coroutines.launch
import mycelium.api.CreateScheduleRequest
import mycelium.api.EachRun
import mycelium.api.Schedule
import mycelium.api.ScheduleRun
import mycelium.state.AppState
import mycelium.state.Store
import mycelium.state.WhenKind
import mycelium.state.Weekdays
import mycelium.state.ago
import mycelium.state.cronFor
import mycelium.state.describeCron
import mycelium.state.eachRunName
import mycelium.state.projectName
import mycelium.state.statusSectionName
import mycelium.state.until

private const val SkippedOutcome = "skipped"

@Composable
fun SchedulesScreen(store: Store, state: AppState) {
    val colors = MyTheme.colors
    var creating by remember { mutableStateOf(false) }

    LaunchedEffect(state.conversations.size) {
        store.act {
            store.refreshProjects()
            store.refreshSchedules()
        }
    }

    val schedules = state.schedules.values.sortedBy { it.createdAt }
    val runs = state.scheduleRuns

    Column(Modifier.fillMaxSize()) {
        Bar {
            BarTitle("Schedules")
            Box(Modifier.weight(1f))
            MyButton({ creating = true }, text = "New schedule", icon = MyIcon.Plus, variant = ButtonVariant.Outline)
        }

        Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(Tokens.Space.S4), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4)) {
            if (schedules.isNotEmpty()) {
                TableCard {
                    Row(Modifier.fillMaxWidth().tableHeader(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3), verticalAlignment = Alignment.CenterVertically) {
                        Label("Prompt", Modifier.weight(3f), Tokens.Type.MetaStrong, colors.textMuted)
                        Label("Project", Modifier.weight(1.5f), Tokens.Type.MetaStrong, colors.textMuted)
                        Label("When", Modifier.weight(1.5f), Tokens.Type.MetaStrong, colors.textMuted)
                        Label("Each run", Modifier.weight(1.5f), Tokens.Type.MetaStrong, colors.textMuted)
                        Label("Next", Modifier.weight(1f), Tokens.Type.MetaStrong, colors.textMuted)
                        Box(Modifier.weight(2.5f))
                    }

                    Hairline()

                    for (schedule in schedules) {
                        ScheduleRow(schedule, store, state)
                        Hairline()
                    }
                }
            }

            if (runs.isNotEmpty()) {
                TableCard {
                    Row(Modifier.fillMaxWidth().tableHeader(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3), verticalAlignment = Alignment.CenterVertically) {
                        Label("Last runs", Modifier.weight(1f).semantics { heading() }, Tokens.Type.MetaStrong, colors.textMuted)
                        Label("Conversation", Modifier.weight(3f), Tokens.Type.MetaStrong, colors.textMuted)
                        Label("Result", Modifier.weight(3f), Tokens.Type.MetaStrong, colors.textMuted)
                    }

                    Hairline()

                    for (run in runs) {
                        RunRow(run, state)
                        Hairline()
                    }
                }
            }

            if (schedules.isEmpty()) {
                Label("No schedule is set up. Add one to start a conversation on its own.", color = colors.textMuted)
            }
        }
    }

    if (creating) {
        NewScheduleDialog(store, state, onDismiss = { creating = false })
    }
}

@Composable
private fun ScheduleRow(schedule: Schedule, store: Store, state: AppState) {
    val colors = MyTheme.colors
    var reviewing by remember(schedule.id) { mutableStateOf(false) }

    Row(
        Modifier.fillMaxWidth().heightIn(min = rowHeight()).padding(horizontal = Tokens.Space.S3),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Label(schedule.prompt, Modifier.weight(3f), maxLines = 1)
        Label(state.projects[schedule.projectId]?.name ?: projectName(schedule.cwd), Modifier.weight(1.5f), color = colors.textMuted, maxLines = 1)
        Column(Modifier.weight(1.5f)) {
            Label(describeCron(schedule.cron), style = Tokens.Type.Code, maxLines = 1)
            Label(schedule.timeZone, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
        }

        Label(eachRunName(schedule.eachRun), Modifier.weight(1.5f), color = colors.textMuted, maxLines = 1)
        Label(if (schedule.proposed) "Waiting for you" else if (schedule.paused) "Paused" else until(schedule.nextRunAt), Modifier.weight(1f), color = if (schedule.paused) colors.textFaint else colors.text, maxLines = 1)

        Row(Modifier.weight(2.5f), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1, Alignment.End)) {
            if (schedule.proposed) {
                MyButton({ reviewing = true }, text = "Review", description = "Review: ${schedule.prompt}", size = ButtonSize.Sm)
                MyButton({ store.act { store.declineSchedule(schedule) } }, text = "Decline", description = "Decline: ${schedule.prompt}", size = ButtonSize.Sm, muted = true)
            } else {
                MyButton({ store.act { store.runSchedule(schedule) } }, text = "Run now", description = "Run now: ${schedule.prompt}", size = ButtonSize.Sm)
                MyButton(
                    { store.act { store.pauseSchedule(schedule, !schedule.paused) } },
                    text = if (schedule.paused) "Resume" else "Pause",
                    description = "${if (schedule.paused) "Resume" else "Pause"}: ${schedule.prompt}",
                    size = ButtonSize.Sm,
                )
                MyButton({ store.act { store.deleteSchedule(schedule) } }, text = "Delete", description = "Delete: ${schedule.prompt}", size = ButtonSize.Sm, muted = true)
            }
        }
    }

    if (reviewing) {
        ProposedScheduleDialog(store, schedule, state, onDismiss = { reviewing = false })
    }
}

@Composable
private fun RunRow(run: ScheduleRun, state: AppState) {
    val colors = MyTheme.colors
    val conversation = state.conversations[run.conversationId]

    Row(
        Modifier.fillMaxWidth().heightIn(min = rowHeight()).padding(horizontal = Tokens.Space.S3),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Label(ago(run.ranAt), Modifier.weight(1f), color = colors.textMuted, maxLines = 1)
        Label(conversation?.title ?: state.schedules[run.scheduleId]?.prompt.orEmpty(), Modifier.weight(3f), maxLines = 1)

        Row(Modifier.weight(3f), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalAlignment = Alignment.CenterVertically) {
            if (run.outcome == SkippedOutcome) {
                StatusMark("Skipped: ${run.reason.orEmpty()}", Tone.Attention, MyIcon.Alert)
            } else if (conversation != null) {
                StatusGlyph(conversation.status)
                Label(statusSectionName(conversation.status), color = colors.text, maxLines = 1)
            }
        }
    }
}

@Composable
private fun NewScheduleDialog(store: Store, state: AppState, onDismiss: () -> Unit) {
    val scope = rememberCoroutineScope()
    val place = remember { Place() }
    val timeZone = LocalPlatform.current?.timeZone() ?: "UTC"
    var prompt by remember { mutableStateOf("") }
    var kind by remember { mutableStateOf(WhenKind.Weekly) }
    var every by remember { mutableStateOf("1") }
    var hours by remember { mutableStateOf(true) }
    var at by remember { mutableStateOf("06:00") }
    var weekday by remember { mutableStateOf(Weekdays.first()) }
    var custom by remember { mutableStateOf("") }
    var eachRun by remember { mutableStateOf(EachRun.New) }
    var promptError by remember { mutableStateOf<String?>(null) }
    var whenError by remember { mutableStateOf<String?>(null) }
    var cwdError by remember { mutableStateOf<String?>(null) }
    var failure by remember { mutableStateOf<String?>(null) }
    var running by remember { mutableStateOf(false) }

    fun submit() {
        val target = place.target(state)

        if (running || target == null) {
            return
        }

        val cron = cronFor(kind, every.trim().toIntOrNull() ?: 0, hours, at, weekday, custom)

        promptError = if (prompt.isBlank()) "Enter the prompt to run on this schedule." else null
        cwdError = if (target.project == null && place.cwd.isBlank()) "Enter the folder the agent should work in, for example /home/you/project." else null
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

        failure = null
        running = true

        scope.launch {
            failure = attempt {
                store.createSchedule(request)
                onDismiss()
            }
            running = false
        }
    }

    Dialog(onDismiss) {
        Label("New schedule", Modifier.semantics { heading() }, Tokens.Type.Title)

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
            TextInput(prompt, { prompt = it }, label = "Prompt", placeholder = "Update and test dependencies", error = promptError)
            PlacePicker(store, state, place, withBranch = false, cwdError = cwdError)

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
            MyButton(::submit, text = "Add schedule", variant = ButtonVariant.Outline)
        }
    }
}
