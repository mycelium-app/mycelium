package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import kotlinx.coroutines.launch
import mycelium.api.MessageProposal
import mycelium.api.Task
import mycelium.api.TaskStatuses
import mycelium.state.AppState
import mycelium.state.MyceliumCall
import mycelium.state.Store
import mycelium.state.ThreadItem
import mycelium.state.ToolStatus

private const val CollapsedLimit = 3
private const val ExcerptLines = 3

class QuestionAnswers(val tasks: Map<String, Task>, val refresh: suspend () -> Unit, val answer: suspend (Task, String) -> Unit)

val LocalQuestionAnswers = compositionLocalOf<QuestionAnswers?> { null }

fun questionAnswers(store: Store, state: AppState) = QuestionAnswers(state.tasks, { store.refreshBoard() }, { task, text -> store.answerTask(task, text) })

class MessageProposals(val proposals: Map<String, MessageProposal>, val refresh: suspend () -> Unit, val approve: suspend (MessageProposal) -> Unit, val decline: suspend (MessageProposal) -> Unit)

val LocalMessageProposals = compositionLocalOf<MessageProposals?> { null }

fun messageProposals(store: Store, state: AppState) = MessageProposals(state.messageProposals, { store.refreshMessageProposals() }, { proposal -> store.approveMessageProposal(proposal) }, { proposal -> store.declineMessageProposal(proposal) })

@Composable
private fun proposedMessage(parsed: MyceliumCall): MessageProposal? {
    val sending = parsed as? MyceliumCall.SendToConversation ?: return null

    val proposals = LocalMessageProposals.current?.proposals ?: return null

    return sending.proposalId?.let { proposals[it] } ?: proposals.values.filter { it.message == sending.message && it.reason == sending.reason }.maxByOrNull { it.createdAt }
}

@Composable
private fun askedTask(parsed: MyceliumCall): Task? {
    val taskId = (parsed as? MyceliumCall.AskQuestion)?.taskId ?: return null

    return LocalQuestionAnswers.current?.tasks?.get(taskId)
}

@Composable
internal fun MyceliumCard(call: ThreadItem.ToolCall, parsed: MyceliumCall) {
    val colors = MyTheme.colors
    val failed = call.status == ToolStatus.Failed
    val finished = call.status == ToolStatus.Done
    val verb = if (call.status == ToolStatus.Running || call.status == ToolStatus.Pending) "${parsed.tool.running}…" else null
    val heading = when {
        failed -> parsed.tool.label
        verb != null -> runningHeading(parsed, verb)
        else -> doneHeading(parsed)
    }
    val icon = when (parsed) {
        is MyceliumCall.Recall, is MyceliumCall.Remember -> MyIcon.Memory
        is MyceliumCall.ProposeTask -> MyIcon.Task
        is MyceliumCall.AskQuestion -> MyIcon.Question
        is MyceliumCall.ProposeSchedule -> MyIcon.Calendar
        is MyceliumCall.Walkthrough -> MyIcon.List
        is MyceliumCall.ProposeInstructionsEdit -> MyIcon.File
        is MyceliumCall.FindConversations -> MyIcon.Search
        is MyceliumCall.SendToConversation -> MyIcon.Send
    }
    val proposal = proposedMessage(parsed)
    val answered = askedTask(parsed)?.status == TaskStatuses.Done
    val waiting = when (parsed) {
        is MyceliumCall.ProposeTask -> "Waiting for approval"
        is MyceliumCall.AskQuestion -> "Waiting for your answer"
        is MyceliumCall.ProposeSchedule -> "Waiting for confirmation"
        is MyceliumCall.ProposeInstructionsEdit -> "Waiting for approval"
        is MyceliumCall.SendToConversation -> "Waiting for approval"
        else -> null
    }

    Card(Modifier.semantics { contentDescription = heading }) {
        CardHead {
            Icon(icon, colors.textMuted, Tokens.Size.IconSm)
            Label(heading, Modifier.weight(1f), Tokens.Type.UiStrong, maxLines = 1)

            when {
                failed -> StatusMark("Failed", Tone.Failed, MyIcon.Failed)
                verb != null && call.awaitingApproval -> StatusMark("Waiting", Tone.Muted, MyIcon.Clock)
                verb != null -> StatusMark("Running", Tone.Working, MyIcon.Working)
                finished && answered -> StatusMark("Answered", Tone.Muted, MyIcon.Check)
                finished && proposal?.state == "sent" -> StatusMark("Sent", Tone.Muted, MyIcon.Check)
                finished && proposal?.state == "declined" -> StatusMark("Declined", Tone.Muted, MyIcon.Close)
                finished && waiting != null -> StatusMark(waiting, Tone.Muted, MyIcon.Clock)
            }
        }

        val error = call.output?.trim()?.takeIf { failed && it.isNotEmpty() }

        if (error != null) {
            Label(error, Modifier.padding(Tokens.Space.S3), Tokens.Type.Ui, colors.statusFailed)
        } else if (verb == null) {
            Column(Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5)) {
                MyceliumBody(parsed)
            }
        }
    }
}

private fun runningHeading(parsed: MyceliumCall, verb: String): String = when {
    parsed is MyceliumCall.Recall && parsed.query.isNotEmpty() -> "${verb.removeSuffix("…")} for “${parsed.query}”…"
    else -> verb
}

private fun doneHeading(parsed: MyceliumCall): String = when (parsed) {
    is MyceliumCall.Recall -> "Searched memory for “${parsed.query}”"
    is MyceliumCall.Remember -> "Remembered"
    is MyceliumCall.ProposeTask -> "Proposed a task"
    is MyceliumCall.AskQuestion -> "Asked a question"
    is MyceliumCall.ProposeSchedule -> "Proposed a schedule"
    is MyceliumCall.ProposeInstructionsEdit -> "Proposed an edit to the ${parsed.layer ?: "agent"} instructions"
    is MyceliumCall.FindConversations -> if (parsed.query != null) "Searched conversations for “${parsed.query}”" else "Listed recent conversations"
    is MyceliumCall.SendToConversation -> if (parsed.target.isNotEmpty()) "Proposed a message to ${parsed.target}" else "Proposed a message"
    is MyceliumCall.Walkthrough -> "Wrote a walkthrough: ${parsed.steps.size} ${if (parsed.steps.size == 1) "step" else "steps"}"
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun MyceliumBody(parsed: MyceliumCall) {
    val colors = MyTheme.colors

    when (parsed) {
        is MyceliumCall.Recall -> {
            if (parsed.memories.isEmpty()) {
                Label("Nothing in memory matches “${parsed.query}”.", style = Tokens.Type.Ui, color = colors.textMuted)
            } else {
                Collapsible(parsed.memories, "memories") { Label(it, style = Tokens.Type.Ui, maxLines = 2) }
            }

            parsed.omitted?.let { Label(it, style = Tokens.Type.Meta, color = colors.textMuted) }
        }

        is MyceliumCall.Remember -> {
            Label(parsed.text, style = Tokens.Type.Ui, maxLines = ExcerptLines)
            parsed.sharedWith?.let { Label("Shared with $it", style = Tokens.Type.Meta, color = colors.textMuted) }
        }

        is MyceliumCall.ProposeTask -> {
            Label(parsed.title, style = Tokens.Type.UiStrong, maxLines = 2)
            parsed.body?.let { Label(it, style = Tokens.Type.Ui, color = colors.textMuted, maxLines = ExcerptLines) }
            parsed.project?.let { Tag(it) }
        }

        is MyceliumCall.AskQuestion -> {
            val answers = LocalQuestionAnswers.current
            val task = askedTask(parsed)

            LaunchedEffect(parsed.taskId, task == null) {
                if (answers != null && parsed.taskId != null && task == null) {
                    answers.refresh()
                }
            }

            Label(parsed.question, style = Tokens.Type.Ui, maxLines = ExcerptLines)

            if (answers != null && task != null && task.status == TaskStatuses.Question) {
                AnswerForm(task, send = { text -> answers.answer(task, text) }, spacing = Tokens.Space.S2)
            } else if (task?.answer != null) {
                Label("Answered: ${task.answer}", style = Tokens.Type.Ui, color = colors.textMuted)
            } else if (parsed.options.isNotEmpty()) {
                FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5)) {
                    for (option in parsed.options) {
                        Tag(option)
                    }
                }
            }
        }

        is MyceliumCall.ProposeSchedule -> {
            Label(parsed.prompt, style = Tokens.Type.Ui, maxLines = ExcerptLines)

            parsed.schedule?.let {
                Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalAlignment = Alignment.CenterVertically) {
                    Icon(MyIcon.Clock, colors.textMuted, Tokens.Size.IconSm)
                    Label(it, style = Tokens.Type.Meta, color = colors.textMuted)
                }
            }
        }

        is MyceliumCall.ProposeInstructionsEdit -> {
            val review = LocalReviewInstructions.current

            Label(parsed.reason, style = Tokens.Type.Ui, maxLines = ExcerptLines)

            if (review != null) {
                MyButton(review, text = "Review", variant = ButtonVariant.Outline, size = ButtonSize.Sm, description = "Review the proposed instructions edit in Settings")
            }
        }

        is MyceliumCall.FindConversations -> {
            if (parsed.conversations.isEmpty()) {
                Label("No conversations match.", style = Tokens.Type.Ui, color = colors.textMuted)
            } else {
                Collapsible(parsed.conversations, "conversations") { Label(it, style = Tokens.Type.Ui, maxLines = 2) }
            }
        }

        is MyceliumCall.SendToConversation -> SendToConversationBody(parsed)

        is MyceliumCall.Walkthrough -> Collapsible(parsed.steps.mapIndexed { index, title -> "${index + 1}. $title" }, "steps") { Label(it, style = Tokens.Type.Ui, maxLines = 1) }
    }
}

@Composable
private fun SendToConversationBody(parsed: MyceliumCall.SendToConversation) {
    val colors = MyTheme.colors
    val proposals = LocalMessageProposals.current
    val proposal = proposedMessage(parsed)
    val scope = rememberCoroutineScope()
    var running by remember { mutableStateOf(false) }
    var failure by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(proposal == null) {
        if (proposals != null && proposal == null) {
            proposals.refresh()
        }
    }

    fun decide(decision: suspend (MessageProposal) -> Unit) {
        val deciding = proposal

        if (running || deciding == null) {
            return
        }

        failure = null
        running = true

        scope.launch {
            failure = attempt { decision(deciding) }
            running = false
        }
    }

    if (proposal != null) {
        val place = listOfNotNull(proposal.targetNode.ifEmpty { null }, proposal.targetProject).joinToString(" · ")

        Label(proposal.targetTitle, style = Tokens.Type.UiStrong, maxLines = 1)

        if (place.isNotEmpty()) {
            Label(place, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
        }
    }

    Label(parsed.message, style = Tokens.Type.Ui, maxLines = ExcerptLines)
    Label(parsed.reason, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = ExcerptLines)

    failure?.let { ErrorBanner(it, onDismiss = { failure = null }) }

    if (proposals != null && proposal != null && proposal.waiting) {
        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5)) {
            MyButton({ decide(proposals.approve) }, text = "Send", variant = ButtonVariant.Primary, size = ButtonSize.Sm)
            MyButton({ decide(proposals.decline) }, text = "Don't send", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
        }
    }
}

@Composable
private fun Collapsible(items: List<String>, noun: String, entry: @Composable (String) -> Unit) {
    var expanded by remember { mutableStateOf(false) }
    val shown = if (expanded) items else items.take(CollapsedLimit)

    for (item in shown) {
        entry(item)
    }

    if (items.size > CollapsedLimit) {
        MyButton(
            onClick = { expanded = !expanded },
            text = if (expanded) "Show fewer" else "Show all ${items.size}",
            description = if (expanded) "Show fewer $noun" else "Show all ${items.size} $noun",
            size = ButtonSize.Sm,
            muted = true,
        )
    }
}
