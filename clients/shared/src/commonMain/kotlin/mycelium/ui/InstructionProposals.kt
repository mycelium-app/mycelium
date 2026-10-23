package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.DisableSelection
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import mycelium.api.InstructionProposal
import mycelium.state.DiffEntry
import mycelium.state.DiffLine
import mycelium.state.Store
import mycelium.state.decidedProposals
import mycelium.state.proposalDiff
import mycelium.state.waitingProposals

val LocalReviewInstructions = staticCompositionLocalOf<(() -> Unit)?> { null }

val LocalOpenConversation = staticCompositionLocalOf<((String) -> Unit)?> { null }

fun waitingInstructionProposals(state: mycelium.state.AppState): Int = state.instructionProposals.values.count { it.waiting }

@Composable
fun CountBadge(count: Int, description: String, modifier: Modifier = Modifier) {
    val colors = MyTheme.colors

    Box(
        modifier
            .background(colors.statusAttention, BlockShape)
            .padding(horizontal = Tokens.Space.S1_5)
            .semantics { contentDescription = description },
        contentAlignment = Alignment.Center,
    ) {
        Label(count.toString(), style = Tokens.Type.MetaStrong, color = colors.bgContent)
    }
}

@Composable
fun InstructionProposals(store: Store) {
    val state by store.state.collectAsState()
    val waiting = waitingProposals(state.instructionProposals.values)
    val decided = decidedProposals(state.instructionProposals.values)

    LaunchedEffect(Unit) {
        store.act { store.refreshInstructionProposals() }
    }

    if (waiting.isEmpty() && decided.isEmpty()) {
        return
    }

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
        Label("Proposed edits", Modifier.semantics { heading() }, Tokens.Type.UiStrong)

        for (proposal in waiting) {
            ProposalCard(store, proposal)
        }

        if (decided.isNotEmpty()) {
            Label("Decided", Modifier.semantics { heading() }, Tokens.Type.MetaStrong, MyTheme.colors.textMuted)

            for (proposal in decided.take(DecidedShown)) {
                DecidedRow(proposal)
            }
        }
    }
}

private const val DecidedShown = 5

@Composable
private fun ProposalCard(store: Store, proposal: InstructionProposal) {
    val colors = MyTheme.colors
    val openConversation = LocalOpenConversation.current
    var declining by remember(proposal.id) { mutableStateOf(false) }
    var note by remember(proposal.id) { mutableStateOf("") }
    val entries = remember(proposal.id, proposal.baseText, proposal.text) { proposalDiff(proposal.baseText, proposal.text) }

    Card(Modifier.semantics { contentDescription = "Proposed edit to the ${proposal.layerName} instructions" }) {
        CardHead {
            Label("${proposal.layerName} instructions", Modifier.weight(1f), Tokens.Type.UiStrong, maxLines = 1)

            if (proposal.outOfDate) {
                StatusMark("Out of date", Tone.Attention, MyIcon.Alert)
            }
        }

        Column(Modifier.padding(Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
            Label(proposal.reason, color = colors.text)

            val conversationId = proposal.conversationId

            if (conversationId != null && openConversation != null) {
                MyButton(
                    { openConversation(conversationId) },
                    text = proposal.conversationTitle?.takeIf { it.isNotBlank() } ?: "Open the conversation",
                    icon = MyIcon.ChevronRight,
                    variant = ButtonVariant.Outline,
                    size = ButtonSize.Sm,
                    description = "Open the conversation that proposed this edit",
                )
            } else if (!proposal.conversationTitle.isNullOrBlank()) {
                Label(proposal.conversationTitle, style = Tokens.Type.Meta, color = colors.textMuted)
            }

            ProposalDiff(entries)

            if (proposal.outOfDate) {
                Label(
                    "The instructions changed after this edit was proposed, so it cannot be approved as it is. Decline it, or ask the agent to propose it again on the current text.",
                    style = Tokens.Type.Meta,
                    color = colors.textMuted,
                )
            }

            if (declining) {
                TextInput(note, { note = it }, "Note to the agent (optional)", Modifier.fillMaxWidth(), placeholder = "Why you are declining it")
            }

            FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                if (declining) {
                    MyButton({ store.act { store.declineInstructionProposal(proposal, note, redo = false) } }, text = "Decline", variant = ButtonVariant.Danger, description = "Decline the proposed edit to the ${proposal.layerName} instructions")
                    MyButton({ declining = false; note = "" }, text = "Cancel", variant = ButtonVariant.Outline)
                } else {
                    if (proposal.outOfDate) {
                        MyButton({ store.act { store.declineInstructionProposal(proposal, null, redo = true) } }, text = "Ask the agent to redo", variant = ButtonVariant.Primary, description = "Decline the out of date edit and ask the agent to propose it again")
                    } else {
                        MyButton({ store.act { store.approveInstructionProposal(proposal) } }, text = "Approve", icon = MyIcon.Check, variant = ButtonVariant.Primary, description = "Approve the proposed edit to the ${proposal.layerName} instructions")
                    }

                    MyButton({ declining = true }, text = "Decline", variant = ButtonVariant.Outline, description = "Decline the proposed edit to the ${proposal.layerName} instructions")
                }
            }
        }
    }
}

@Composable
private fun ProposalDiff(entries: List<DiffEntry>) {
    val colors = MyTheme.colors

    Column(Modifier.fillMaxWidth().background(colors.bgPanel, BlockShape).padding(vertical = Tokens.Space.S1).horizontalScroll(rememberScrollState())) {
        for (entry in entries) {
            when (entry) {
                is DiffEntry.Line -> ProposalDiffRow(entry.line)
                is DiffEntry.Skipped -> Label("${entry.count} unchanged ${if (entry.count == 1) "line" else "lines"}", Modifier.padding(horizontal = Tokens.Space.S2), Tokens.Type.Meta, colors.textFaint)
            }
        }
    }
}

@Composable
private fun ProposalDiffRow(line: DiffLine) {
    val colors = MyTheme.colors
    val background = when (line.kind) {
        DiffLine.Kind.Added -> colors.diffAddBg
        DiffLine.Kind.Removed -> colors.diffDelBg
        DiffLine.Kind.Context -> Color.Transparent
    }
    val (sign, signColor, meaning) = when (line.kind) {
        DiffLine.Kind.Added -> Triple("+", colors.diffAdd, "Added: ")
        DiffLine.Kind.Removed -> Triple("-", colors.diffDel, "Removed: ")
        DiffLine.Kind.Context -> Triple("", colors.textFaint, "")
    }

    Row(Modifier.background(background).semantics(mergeDescendants = true) { contentDescription = meaning + line.text.ifEmpty { "blank line" } }) {
        DisableSelection {
            Label(line.oldNumber?.toString() ?: "", Modifier.width(Tokens.Size.DiffNumber).padding(end = Tokens.Space.S2), Tokens.Type.CodeSm.copy(textAlign = TextAlign.End), colors.textFaint)
            Label(line.newNumber?.toString() ?: "", Modifier.width(Tokens.Size.DiffNumber).padding(end = Tokens.Space.S2), Tokens.Type.CodeSm.copy(textAlign = TextAlign.End), colors.textFaint)
            Label(sign, Modifier.width(Tokens.Size.DiffSign), Tokens.Type.Code, signColor)
        }

        Label(line.text, style = Tokens.Type.Code)
    }
}

@Composable
private fun DecidedRow(proposal: InstructionProposal) {
    val colors = MyTheme.colors
    val outcome = when (proposal.state) {
        "approved" -> "Approved"
        "declined" -> "Declined"
        else -> "Replaced by a newer proposal"
    }

    Column(Modifier.fillMaxWidth().semantics(mergeDescendants = true) {}, verticalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5)) {
        Label("$outcome: ${proposal.layerName} instructions", maxLines = 1)
        Label(proposal.note?.let { "Your note: $it" } ?: proposal.reason, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 2)
    }
}
