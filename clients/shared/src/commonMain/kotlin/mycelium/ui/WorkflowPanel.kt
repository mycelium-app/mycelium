package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import mycelium.api.Conversation
import mycelium.api.Workflow
import mycelium.api.WorkflowControlRequest
import mycelium.state.WorkflowActions
import mycelium.state.WorkflowStepStatuses
import mycelium.state.ago
import mycelium.state.ended
import mycelium.state.runMinutes
import mycelium.state.subagentConversation

private data class PanelStep(val key: String, val title: String, val status: String, val conversationId: String?, val time: String?)

@Composable
internal fun WorkflowPanelDetail(workflow: Workflow, conversations: Collection<Conversation>, onBack: () -> Unit, onOpenConversation: (String) -> Unit, onControl: (WorkflowControlRequest) -> Unit) {
    val colors = MyTheme.colors
    var declining by remember(workflow.id) { mutableStateOf<GateChoice?>(null) }
    val idleStatus = if (workflow.ended()) WorkflowStepStatuses.NotRun else WorkflowStepStatuses.Waiting
    val steps = if (workflow.plan.steps.isNotEmpty()) {
        workflow.plan.steps.map { spec ->
            val step = workflow.steps.firstOrNull { it.key == spec.key }

            PanelStep(spec.key, spec.title, step?.status ?: idleStatus, step?.conversationId, runMinutes(step?.startedAt, step?.endedAt))
        }
    } else {
        workflow.steps.map { step ->
            val conversation = step.agentId?.let { workflow.subagentConversation(conversations, it) }

            PanelStep(step.key, step.label ?: step.key, step.status, conversation?.id ?: step.conversationId, runMinutes(step.startedAt, step.endedAt))
        }
    }

    Column(Modifier.fillMaxSize().background(colors.bgPanel).semantics { contentDescription = "Workflow" }) {
        Bar(startPadding = Tokens.Space.S1) {
            BackButton("subagents and workflows", onBack)
            Icon(MyIcon.Workflow, colors.textMuted, Tokens.Size.IconSm)
            BarTitle(workflow.title)
        }

        Column(Modifier.weight(1f).verticalScrollWithBar().padding(vertical = Tokens.Space.S2)) {
            Label("Workflow · started ${ago(workflow.createdAt)}", Modifier.padding(horizontal = Tokens.Space.S3), Tokens.Type.Meta, colors.textMuted)

            steps.forEachIndexed { index, step ->
                val look = stepStatusLook(step.status)
                val open = step.conversationId

                if (step.status == WorkflowStepStatuses.Gate && !workflow.ended()) {
                    Column(
                        Modifier
                            .fillMaxWidth()
                            .padding(horizontal = Tokens.Space.S1_5, vertical = Tokens.Space.S1)
                            .background(colors.bgContent, BlockShape)
                            .border(Tokens.Size.Hairline, colors.border, BlockShape)
                            .padding(Tokens.Space.S2)
                            .semantics { contentDescription = "Gate, ${step.title}, waiting for you" },
                        verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                    ) {
                        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.Top) {
                            Icon(look.icon, look.tone.color(), Tokens.Size.IconSm)

                            Column(Modifier.weight(1f)) {
                                Label(step.title)
                                Label("Gate · ${look.label}", style = Tokens.Type.Meta, color = colors.textMuted)
                            }
                        }

                        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End)) {
                            MyButton({ declining = GateChoice(step.key, sendBack = true) }, text = "Decline", description = "Decline ${step.title}", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
                            MyButton({ onControl(WorkflowControlRequest(WorkflowActions.Approve, step.key)) }, text = "Approve", description = "Approve ${step.title}", variant = ButtonVariant.Primary, size = ButtonSize.Sm)
                        }
                    }
                } else {
                    val started = step.status != WorkflowStepStatuses.Waiting && step.status != WorkflowStepStatuses.NotRun
                    val description = "Step ${index + 1}, ${step.title}, ${look.label}" + if (open != null) ", open its conversation" else ""

                    Row(
                        Modifier
                            .fillMaxWidth()
                            .then(if (open != null) Modifier.pressable({ onOpenConversation(open) }, shape = BlockShape, description = description) else Modifier.semantics(mergeDescendants = true) { contentDescription = description })
                            .padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S1_5),
                        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Icon(look.icon, if (started) look.tone.color() else colors.textFaint, Tokens.Size.IconSm)

                        Column(Modifier.weight(1f)) {
                            Label(step.title, color = if (started) colors.text else colors.textMuted, maxLines = 1)
                            Label(look.label, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
                        }

                        step.time?.let { Label(it, style = Tokens.Type.Meta, color = colors.textFaint, maxLines = 1) }

                        if (open != null) {
                            Icon(MyIcon.ChevronRight, colors.textMuted, Tokens.Size.IconSm)
                        }
                    }
                }
            }
        }
    }

    declining?.let { choice ->
        GateDecisionDialog(workflow, choice, { declining = null }) { request ->
            declining = null
            onControl(request)
        }
    }
}
