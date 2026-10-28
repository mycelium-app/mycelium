package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import mycelium.api.Workflow
import mycelium.api.WorkflowControlRequest
import mycelium.state.WorkflowActions
import mycelium.state.firstStepTitle
import mycelium.state.lastRound
import mycelium.state.stepTitle

data class GateChoice(val stepKey: String, val sendBack: Boolean)

const val SendBackReasonRequired = "Enter what needs to change, so the next round can fix it"

@Composable
fun StopWorkflowDialog(title: String, onDismiss: () -> Unit, onConfirm: () -> Unit) {
    ConfirmDialog(
        "Stop $title?",
        "Steps that are running stop where they are and the rest do not run. Their conversations stay, and you can re-run the workflow later.",
        "Stop workflow",
        onDismiss,
        onConfirm,
    )
}

@Composable
fun GateDecisionDialog(workflow: Workflow, choice: GateChoice, onDismiss: () -> Unit, onDecide: (WorkflowControlRequest) -> Unit) {
    val title = workflow.stepTitle(choice.stepKey)
    var text by remember { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }

    Dialog(onDismiss) {
        if (choice.sendBack) {
            val last = workflow.lastRound()
            val max = workflow.plan.maxRounds
            val next = workflow.round + 1

            Label("Send $title back?", Modifier.semantics { heading() }, Tokens.Type.Title)
            Label(
                if (last) {
                    "Round $max of $max is the last round, so sending $title back ends the workflow. Your reason is kept with the decision."
                } else {
                    "Round $next of $max starts from ${workflow.firstStepTitle()}, and every step gets round ${workflow.round}’s results and your reason." + if (next >= max) " Round $max is the last: its $title ends the workflow either way." else ""
                },
                style = Tokens.Type.Meta,
                color = MyTheme.colors.textMuted,
            )
            TextArea(text, { text = it; error = null }, "What needs to change", error = error)
            Label("The agents see this word for word.", style = Tokens.Type.Meta, color = MyTheme.colors.textMuted)
        } else {
            Label("Approve $title?", Modifier.semantics { heading() }, Tokens.Type.Title)
            Label("The workflow carries on from here. Add a note if the next steps should know why.", style = Tokens.Type.Meta, color = MyTheme.colors.textMuted)
            TextArea(text, { text = it }, "Note (optional)", minLines = 3)
        }

        FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            MyButton(onDismiss, text = "Cancel", muted = true)
            MyButton(
                {
                    if (choice.sendBack && text.isBlank()) {
                        error = SendBackReasonRequired
                    } else if (choice.sendBack) {
                        onDecide(WorkflowControlRequest(WorkflowActions.SendBack, choice.stepKey, reason = text.trim()))
                    } else {
                        onDecide(WorkflowControlRequest(WorkflowActions.Approve, choice.stepKey, reason = text.trim().takeIf { it.isNotEmpty() }))
                    }
                },
                text = if (choice.sendBack) "Send back" else "Approve",
                description = if (choice.sendBack) (if (workflow.lastRound()) "Send back and end the workflow" else "Send back and start round ${workflow.round + 1}") else "Approve and continue",
                variant = ButtonVariant.Primary,
            )
        }
    }
}
