package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import kotlinx.coroutines.launch
import mycelium.api.ConfirmScheduleRequest
import mycelium.api.EachRun
import mycelium.api.Schedule
import mycelium.state.AppState
import mycelium.state.Store
import mycelium.state.cronTime
import mycelium.state.cronWithTime
import mycelium.state.describeInWords
import mycelium.state.eachRunName
import mycelium.state.projectName
import mycelium.state.until

private fun whereItRuns(schedule: Schedule, state: AppState): String {
    val folder = state.projects[schedule.projectId]?.name ?: projectName(schedule.cwd)
    val node = state.nodes[schedule.nodeId]?.name

    return if (node == null) folder else "$folder on $node"
}

@Composable
fun ProposedScheduleCard(store: Store, schedule: Schedule, state: AppState) {
    val colors = MyTheme.colors
    var reviewing by remember(schedule.id) { mutableStateOf(false) }

    Column(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2)
            .background(colors.bgRaised, FloatShape)
            .border(Tokens.Size.Hairline, colors.border, FloatShape)
            .padding(Tokens.Space.S3)
            .semantics { contentDescription = "Proposed schedule" },
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
    ) {
        Label("Schedule proposed", Modifier.semantics { heading() }, Tokens.Type.MetaStrong, colors.textMuted)
        Label(schedule.prompt, maxLines = 3)
        Label("${describeInWords(schedule.cron, schedule.timeZone)}. ${eachRunName(schedule.eachRun)}. Next run ${until(schedule.nextRunAt)}, ${whereItRuns(schedule, state)}.", color = colors.textMuted)

        FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            MyButton({ store.act { store.confirmSchedule(schedule, ConfirmScheduleRequest()) } }, text = "Confirm schedule", variant = ButtonVariant.Outline)
            MyButton({ reviewing = true }, text = "Change", description = "Change the proposed schedule")
            MyButton({ store.act { store.declineSchedule(schedule) } }, text = "Decline", description = "Decline the proposed schedule", muted = true)
        }
    }

    if (reviewing) {
        ProposedScheduleDialog(store, schedule, state, onDismiss = { reviewing = false })
    }
}

@Composable
fun ProposedScheduleDialog(store: Store, schedule: Schedule, state: AppState, onDismiss: () -> Unit) {
    val scope = rememberCoroutineScope()
    val simpleTime = cronTime(schedule.cron)
    var at by remember { mutableStateOf(simpleTime.orEmpty()) }
    var custom by remember { mutableStateOf(schedule.cron) }
    var zone by remember { mutableStateOf(schedule.timeZone) }
    var eachRun by remember { mutableStateOf(schedule.eachRun) }
    var whenError by remember { mutableStateOf<String?>(null) }
    var failure by remember { mutableStateOf<String?>(null) }
    var running by remember { mutableStateOf(false) }

    fun cron(): String? = if (simpleTime == null) custom.trim().takeIf { it.isNotEmpty() } else cronWithTime(schedule.cron, at)

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

    fun confirm() {
        val edited = cron()

        whenError = if (edited != null) null else if (simpleTime == null) "Enter the schedule as five cron fields, for example 0 20 * * 1-5." else "Enter the time as hours and minutes, for example 20:00."

        if (running || edited == null) {
            return
        }

        val request = ConfirmScheduleRequest(
            cron = edited.takeIf { it != schedule.cron },
            eachRun = eachRun.takeIf { it != schedule.eachRun },
            timeZone = zone.trim().takeIf { it != schedule.timeZone },
        )

        finish { store.confirmSchedule(schedule, request) }
    }

    Dialog(onDismiss) {
        Label("Confirm schedule", Modifier.semantics { heading() }, Tokens.Type.Title)

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                Label("Runs", style = Tokens.Type.MetaStrong, color = MyTheme.colors.textMuted)
                Label(schedule.prompt, maxLines = 6)
                Label(whereItRuns(schedule, state), color = MyTheme.colors.textMuted)
            }

            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                Label("When", style = Tokens.Type.MetaStrong, color = MyTheme.colors.textMuted)
                Label("${describeInWords(cron() ?: schedule.cron, zone.trim())}. Next run ${until(schedule.nextRunAt)}.")
            }

            if (simpleTime == null) {
                TextInput(custom, { custom = it }, label = "Cron expression", mono = true, error = whenError)
            } else {
                TextInput(at, { at = it }, label = "At", placeholder = "20:00", mono = true, error = whenError)
            }

            TextInput(zone, { zone = it }, label = "Time zone", placeholder = "Europe/London", mono = true)

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

        FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            MyButton(onDismiss, text = "Cancel", muted = true)
            MyButton({ finish { store.declineSchedule(schedule) } }, text = "Decline", description = "Decline the proposed schedule", muted = true)
            MyButton(::confirm, text = "Confirm", description = "Confirm the schedule", variant = ButtonVariant.Outline)
        }
    }
}
