package mycelium.ui

import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import mycelium.api.WorkflowStatuses
import mycelium.state.WorkflowStepStatuses
import mycelium.state.clockTime

data class WorkflowStatusLook(val label: String, val icon: MyIcon, val tone: Tone)

fun workflowStatusLook(status: String, stalled: Boolean = false): WorkflowStatusLook = when {
    stalled -> WorkflowStatusLook("Stalled", MyIcon.Alert, Tone.Attention)
    status == WorkflowStatuses.Running -> WorkflowStatusLook("Running", MyIcon.Working, Tone.Working)
    status == WorkflowStatuses.Waiting -> WorkflowStatusLook("Waiting for you", MyIcon.Hand, Tone.Attention)
    status == WorkflowStatuses.Paused -> WorkflowStatusLook("Paused", MyIcon.Clock, Tone.Attention)
    status == WorkflowStatuses.Done -> WorkflowStatusLook("Finished", MyIcon.Check, Tone.Muted)
    status == WorkflowStatuses.Failed -> WorkflowStatusLook("Failed", MyIcon.Failed, Tone.Failed)
    status == WorkflowStatuses.Stopped -> WorkflowStatusLook("Stopped", MyIcon.Stop, Tone.Muted)
    else -> WorkflowStatusLook(status.replace('_', ' ').replaceFirstChar { it.uppercase() }, MyIcon.Minus, Tone.Muted)
}

fun stepStatusLook(status: String, waitingFor: String? = null): WorkflowStatusLook = when (status) {
    WorkflowStepStatuses.Waiting -> WorkflowStatusLook(if (waitingFor == null) "Waiting" else "Waiting for $waitingFor", MyIcon.Clock, Tone.Muted)
    WorkflowStepStatuses.Starting, WorkflowStepStatuses.Running -> WorkflowStatusLook("Running", MyIcon.Working, Tone.Working)
    WorkflowStepStatuses.Gate -> WorkflowStatusLook("Waiting for you", MyIcon.Hand, Tone.Attention)
    WorkflowStepStatuses.Approved -> WorkflowStatusLook("Approved", MyIcon.Check, Tone.Muted)
    WorkflowStepStatuses.SentBack -> WorkflowStatusLook("Sent back", MyIcon.Back, Tone.Muted)
    WorkflowStepStatuses.Done -> WorkflowStatusLook("Finished", MyIcon.Check, Tone.Muted)
    WorkflowStepStatuses.Failed -> WorkflowStatusLook("Failed", MyIcon.Failed, Tone.Failed)
    WorkflowStepStatuses.Stopped -> WorkflowStatusLook("Stopped", MyIcon.Stop, Tone.Muted)
    WorkflowStepStatuses.NotRun -> WorkflowStatusLook("Did not run", MyIcon.Minus, Tone.Muted)
    WorkflowStepStatuses.Paused -> WorkflowStatusLook("Paused", MyIcon.Clock, Tone.Attention)
    else -> WorkflowStatusLook(status.replace('_', ' ').replaceFirstChar { it.uppercase() }, MyIcon.Minus, Tone.Muted)
}

@Composable
fun WorkflowStatusMark(look: WorkflowStatusLook, modifier: Modifier = Modifier, text: String = look.label) {
    StatusMark(text, look.tone, look.icon, modifier.semantics(mergeDescendants = true) { contentDescription = "Status: $text" })
}

@Composable
fun workflowClock(): (String?) -> String {
    val platform = LocalPlatform.current

    return { iso -> if (iso == null) "" else clockTime(iso) { millis -> platform?.localDateTime(millis) } }
}

@Composable
fun Tone.color() = when (this) {
    Tone.Working -> MyTheme.colors.statusWorking
    Tone.Attention -> MyTheme.colors.statusAttention
    Tone.Failed -> MyTheme.colors.statusFailed
    Tone.Muted -> MyTheme.colors.textMuted
}
