package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import mycelium.api.BrowserTask
import mycelium.api.BrowserTaskState
import mycelium.api.Environment
import mycelium.state.AppState
import mycelium.state.Store
import mycelium.state.ThreadItem
import mycelium.state.ToolStatus
import mycelium.state.browserTaskSeconds
import mycelium.state.browserTaskStepsTotal
import mycelium.state.browserTaskStopped

class BrowserTasks(val environments: Map<String, Environment>, val stop: (String) -> Unit)

val LocalBrowserTasks = compositionLocalOf<BrowserTasks?> { null }

fun browserTasks(store: Store, state: AppState) = BrowserTasks(state.environments, { store.stopBrowserTask(it) })

@Composable
internal fun BrowserTaskCard(call: ThreadItem.ToolCall, goal: String) {
    val colors = MyTheme.colors
    val tasks = LocalBrowserTasks.current
    val environment = tasks?.environments?.values?.filter { it.browserTask?.goal == goal }?.maxByOrNull { it.startedAt }
    val task = environment?.browserTask
    var stepsOpen by remember(call.key) { mutableStateOf(false) }
    val running = task?.state == BrowserTaskState.Running || (task == null && (call.status == ToolStatus.Running || call.status == ToolStatus.Pending))

    Card(Modifier.semantics { contentDescription = "Browser task, $goal" }) {
        CardHead {
            Icon(MyIcon.Globe, colors.textMuted, Tokens.Size.IconSm)
            Label("Browser task", style = Tokens.Type.UiStrong, maxLines = 1)
            Label("environment", Modifier.weight(1f), Tokens.Type.Meta, colors.textMuted, maxLines = 1)

            when {
                task?.state == BrowserTaskState.Failed || (task == null && call.status == ToolStatus.Failed) -> StatusMark("Failed", Tone.Failed, MyIcon.Failed)
                running -> StatusMark("Running", Tone.Working, MyIcon.Working)
                task?.state == BrowserTaskState.Blocked -> StatusMark("Blocked", Tone.Attention, MyIcon.Clock)
                task?.state == BrowserTaskState.Stopped -> StatusMark("Stopped", Tone.Muted, MyIcon.Stop)
                else -> StatusMark("Done", Tone.Muted, MyIcon.Check)
            }

            if (running && environment != null && tasks != null) {
                MyButton({ tasks.stop(environment.conversationId) }, text = "Stop task", size = ButtonSize.Sm, variant = ButtonVariant.Outline)
            }
        }

        Column(Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5)) {
            Label(goal, style = Tokens.Type.Ui)

            if (task == null) {
                call.output?.trim()?.takeIf { it.isNotEmpty() && !running }?.let { Label(it, style = Tokens.Type.Ui, color = colors.textMuted) }

                return@Column
            }

            if (running) {
                BrowserTaskSteps(task)
                Label("${task.provider} picks each step · ${browserTaskStepsTotal(task)} so far", Modifier.semantics { liveRegion = LiveRegionMode.Polite }, Tokens.Type.Meta, colors.textMuted)

                return@Column
            }

            when (task.state) {
                BrowserTaskState.Done -> task.result?.let { Label(it, style = Tokens.Type.Ui) }
                BrowserTaskState.Blocked -> Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5)) {
                    Label("Why:", style = Tokens.Type.UiStrong, color = colors.statusAttention)
                    Label(task.why.orEmpty(), Modifier.weight(1f), Tokens.Type.Ui)
                }

                BrowserTaskState.Stopped -> Label(browserTaskStopped(task), style = Tokens.Type.Ui, color = colors.textMuted)
                BrowserTaskState.Failed -> Label(task.why.orEmpty(), style = Tokens.Type.Ui, color = colors.statusFailed)
                BrowserTaskState.Running -> Unit
            }

            if (task.steps.isNotEmpty()) {
                MyButton(
                    { stepsOpen = !stepsOpen },
                    text = "Steps, ${browserTaskStepsTotal(task)}",
                    icon = if (stepsOpen) MyIcon.ChevronDown else MyIcon.ChevronRight,
                    size = ButtonSize.Sm,
                    muted = true,
                    stateDescription = if (stepsOpen) "Shown" else "Hidden",
                )

                if (stepsOpen) {
                    BrowserTaskSteps(task)
                }
            }
        }
    }
}

@Composable
private fun BrowserTaskSteps(task: BrowserTask) {
    val colors = MyTheme.colors

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5)) {
        task.steps.forEachIndexed { index, step ->
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                Label("${index + 1}", style = Tokens.Type.Meta, color = colors.textFaint)
                Label(step.label, Modifier.weight(1f), Tokens.Type.Ui, maxLines = 1)
                Label(browserTaskSeconds(step.millis), style = Tokens.Type.Meta, color = colors.textMuted)
            }
        }
    }
}
