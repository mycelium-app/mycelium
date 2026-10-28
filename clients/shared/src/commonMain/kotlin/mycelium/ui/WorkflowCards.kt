package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
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
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import mycelium.api.Workflow
import mycelium.api.WorkflowControlRequest
import mycelium.state.WorkflowActions
import mycelium.state.WorkflowStepStatuses
import mycelium.state.agentName
import mycelium.state.ended
import mycelium.state.runMinutes
import mycelium.state.stepTitle

data class ConversationWorkflows(
    val workflows: List<Workflow>,
    val onOpen: ((String) -> Unit)?,
    val onControl: (workflowId: String, request: WorkflowControlRequest) -> Unit,
)

val LocalConversationWorkflows = compositionLocalOf<ConversationWorkflows?> { null }

@Composable
internal fun MyceliumWorkflowCard(workflow: Workflow) {
    val pages = LocalConversationWorkflows.current ?: return
    val colors = MyTheme.colors
    val look = workflowStatusLook(workflow.status)
    val finished = workflow.steps.count { it.status == WorkflowStepStatuses.Done || it.status == WorkflowStepStatuses.Approved }
    var stopping by remember(workflow.id) { mutableStateOf(false) }
    var deciding by remember(workflow.id) { mutableStateOf<GateChoice?>(null) }
    var choosing by remember(workflow.id) { mutableStateOf<String?>(null) }
    val open = pages.onOpen

    Card {
        CardHead {
            Icon(MyIcon.Workflow, colors.textMuted, Tokens.Size.IconSm)

            if (open != null) {
                MyButton({ open(workflow.id) }, text = workflow.title, description = "Open ${workflow.title}", size = ButtonSize.Sm, muted = true)
            } else {
                Label(workflow.title, Modifier.semantics { heading() }, Tokens.Type.UiStrong)
            }

            Label("$finished of ${workflow.plan.steps.size}", Modifier.weight(1f), Tokens.Type.Meta, colors.textMuted, maxLines = 1)
            WorkflowStatusMark(look)

            if (!workflow.ended()) {
                MyButton({ stopping = true }, text = "Stop", icon = MyIcon.Stop, description = "Stop ${workflow.title}", size = ButtonSize.Sm, muted = true)
            }
        }

        Column(Modifier.padding(vertical = Tokens.Space.S1)) {
            for (spec in workflow.plan.steps) {
                val step = workflow.steps.firstOrNull { it.key == spec.key }
                val status = step?.status ?: WorkflowStepStatuses.Waiting
                val stepLook = stepStatusLook(status)
                val meta = listOfNotNull(spec.agent?.let(::agentName), runMinutes(step?.startedAt, step?.endedAt)).joinToString(", ")

                Row(
                    Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S0_5),
                    horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Icon(stepLook.icon, stepLook.tone.color(), Tokens.Size.IconSm)
                    Label(spec.title, Modifier.weight(1f).semantics { contentDescription = "${stepLook.label}: ${spec.title}" }, maxLines = 1)

                    if (meta.isNotEmpty()) {
                        Label(meta, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
                    }

                    if (status == WorkflowStepStatuses.Gate) {
                        Box {
                            MyButton({ choosing = spec.key }, text = "Decide", description = "Decide ${spec.title}", variant = ButtonVariant.Outline, size = ButtonSize.Sm, trailingIcon = MyIcon.ChevronDown)

                            if (choosing == spec.key) {
                                MenuPopup(onDismiss = { choosing = null }) {
                                    MenuItem("Approve", onClick = { choosing = null; deciding = GateChoice(spec.key, sendBack = false) }, leading = MyIcon.Check)
                                    MenuItem("Send back", onClick = { choosing = null; deciding = GateChoice(spec.key, sendBack = true) }, leading = MyIcon.Back)
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if (stopping) {
        StopWorkflowDialog(workflow.title, { stopping = false }) {
            stopping = false
            pages.onControl(workflow.id, WorkflowControlRequest(WorkflowActions.Stop))
        }
    }

    deciding?.let { choice ->
        GateDecisionDialog(workflow, choice, { deciding = null }) { request ->
            deciding = null
            pages.onControl(workflow.id, request)
        }
    }
}
