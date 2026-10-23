package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
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
import mycelium.state.MyceliumCall
import mycelium.state.ThreadItem
import mycelium.state.ToolStatus

private const val CollapsedLimit = 3
private const val ExcerptLines = 3

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
    }
    val waiting = when (parsed) {
        is MyceliumCall.ProposeTask -> "Waiting for approval"
        is MyceliumCall.AskQuestion -> "Waiting for your answer"
        is MyceliumCall.ProposeSchedule -> "Waiting for confirmation"
        else -> null
    }

    Card(Modifier.semantics { contentDescription = heading }) {
        CardHead {
            Icon(icon, colors.textMuted, Tokens.Size.IconSm)
            Label(heading, Modifier.weight(1f), Tokens.Type.UiStrong, maxLines = 1)

            when {
                failed -> StatusMark("Failed", Tone.Failed, MyIcon.Failed)
                verb != null -> StatusMark("Running", Tone.Working, MyIcon.Working)
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
            Label(parsed.question, style = Tokens.Type.Ui, maxLines = ExcerptLines)

            if (parsed.options.isNotEmpty()) {
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

        is MyceliumCall.Walkthrough -> Collapsible(parsed.steps.mapIndexed { index, title -> "${index + 1}. $title" }, "steps") { Label(it, style = Tokens.Type.Ui, maxLines = 1) }
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
