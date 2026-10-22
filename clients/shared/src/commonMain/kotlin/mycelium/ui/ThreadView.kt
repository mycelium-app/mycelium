package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.withStyle
import androidx.compose.foundation.text.BasicText
import androidx.compose.foundation.text.selection.SelectionContainer
import mycelium.state.DiffLine
import mycelium.state.FileDiff
import mycelium.state.PlanStatus
import mycelium.state.ThreadItem
import mycelium.state.ToolStatus
import mycelium.state.WorkflowProgress

private const val ShortOutputLimit = 40

@Composable
fun ThreadView(
    items: List<ThreadItem>,
    agentLine: String,
    onAnswer: (requestId: String, optionId: String) -> Unit,
    onStopWorkflow: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val state = rememberLazyListState()

    LaunchedEffect(items.size) {
        if (items.isNotEmpty()) {
            state.scrollToItem(items.lastIndex)
        }
    }

    Box(modifier.fillMaxWidth(), contentAlignment = Alignment.TopCenter) {
        LazyColumn(
            Modifier.widthIn(max = Tokens.Size.Measure).fillMaxWidth().semantics { contentDescription = "Conversation" },
            state = state,
            contentPadding = PaddingValues(
                start = Tokens.Space.S4,
                top = Tokens.Space.S8,
                end = Tokens.Space.S4,
                bottom = Tokens.Space.S4,
            ),
            verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4),
        ) {
            items(items, key = { it.key }) { item ->
                ThreadRow(item, agentLine, onAnswer, onStopWorkflow)
            }
        }
    }
}

@Composable
private fun ThreadRow(item: ThreadItem, agentLine: String, onAnswer: (String, String) -> Unit, onStopWorkflow: () -> Unit) {
    when (item) {
        is ThreadItem.UserMessage -> UserMessage(item.text)
        is ThreadItem.AgentMessage -> AgentMessage(item.text)
        is ThreadItem.ToolCall -> ToolCall(item)
        is ThreadItem.Plan -> PlanCard(item)
        is ThreadItem.Permission -> PermissionCard(item, onAnswer)
        is ThreadItem.TurnEnd -> TurnEnd(item, agentLine)
        is ThreadItem.Failure -> StatusMark(item.message, Tone.Failed, MyIcon.Failed, Modifier.semantics { contentDescription = "Error: ${item.message}" })
        is ThreadItem.Workflow -> WorkflowCard(item.progress, onStopWorkflow)
    }
}

@Composable
private fun UserMessage(text: String) {
    val colors = MyTheme.colors

    SelectionContainer {
        Box(
            Modifier
                .fillMaxWidth()
                .background(colors.bgPanel, BlockShape)
                .border(Tokens.Size.Hairline, colors.border, BlockShape)
                .padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2),
        ) {
            Label(text, style = Tokens.Type.Prose)
        }
    }
}

@Composable
private fun AgentMessage(text: String) {
    val colors = MyTheme.colors

    SelectionContainer {
        Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
            for (block in splitBlocks(text)) {
                if (block.code) {
                    Box(
                        Modifier
                            .fillMaxWidth()
                            .background(colors.bgPanel, BlockShape)
                            .border(Tokens.Size.Hairline, colors.border, BlockShape)
                            .horizontalScroll(rememberScrollState())
                            .padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2),
                    ) {
                        Label(block.text, style = Tokens.Type.Code)
                    }
                } else {
                    BasicText(inlineMarkup(block.text, colors.text, colors.accent), style = Tokens.Type.Prose)
                }
            }
        }
    }
}

data class TextBlock(val text: String, val code: Boolean)

fun splitBlocks(text: String): List<TextBlock> {
    val blocks = mutableListOf<TextBlock>()
    val parts = text.split("```")

    parts.forEachIndexed { index, part ->
        if (index % 2 == 1) {
            val body = part.substringAfter('\n', part).trimEnd('\n')
            blocks += TextBlock(body, code = true)
        } else {
            part.split("\n\n").map { it.trim('\n') }.filter { it.isNotBlank() }.forEach { blocks += TextBlock(it, code = false) }
        }
    }

    return blocks
}

fun inlineMarkup(text: String, color: Color, codeColor: Color): AnnotatedString =
    buildAnnotatedString {
        withStyle(SpanStyle(color = color)) {
            var index = 0

            while (index < text.length) {
                val code = text.startsWith("`", index)
                val bold = text.startsWith("**", index)
                val marker = if (code) "`" else if (bold) "**" else null
                val close = if (marker != null) text.indexOf(marker, index + marker.length) else -1

                if (marker == null || close < 0) {
                    append(text[index])
                    index++
                    continue
                }

                val inner = text.substring(index + marker.length, close)

                if (code) {
                    withStyle(SpanStyle(color = codeColor, fontFamily = Tokens.Type.Code.fontFamily, fontSize = Tokens.Type.Code.fontSize)) { append(inner) }
                } else {
                    withStyle(SpanStyle(fontWeight = FontWeight.SemiBold)) { append(inner) }
                }

                index = close + marker.length
            }
        }
    }

fun toolLabel(kind: String, title: String): String = when (kind) {
    "execute" -> "bash"
    "read", "edit", "search", "fetch", "delete", "move", "think" -> kind
    else -> title.substringBefore(' ').lowercase()
}

@Composable
private fun ToolCall(call: ThreadItem.ToolCall) {
    val colors = MyTheme.colors
    val diff = call.diff

    if (diff != null && call.kind == "edit") {
        DiffCard(diff, call.status)
        return
    }

    Row(
        Modifier.fillMaxWidth().heightIn(min = Tokens.Size.ControlSm),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Label(toolLabel(call.kind, call.title), style = Tokens.Type.Code, color = colors.text, maxLines = 1)
        Label(call.argument, Modifier.weight(1f), Tokens.Type.Code, colors.textMuted, maxLines = 1)

        when (call.status) {
            ToolStatus.Running -> StatusMark("Running", Tone.Working, MyIcon.Working)
            ToolStatus.Failed -> StatusMark("Failed", Tone.Failed, MyIcon.Failed)
            else -> {
                val summary = call.output?.lineSequence()?.firstOrNull()?.takeIf { it.isNotBlank() && it.length <= ShortOutputLimit }

                if (summary != null) {
                    Label(summary, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
                }
            }
        }
    }
}

@Composable
private fun DiffStat(diff: FileDiff) {
    val colors = MyTheme.colors

    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
        Label("+${diff.added}", style = Tokens.Type.Code, color = colors.diffAdd)
        Label("-${diff.removed}", style = Tokens.Type.Code, color = colors.diffDel)
    }
}

@Composable
private fun Card(modifier: Modifier = Modifier, content: @Composable () -> Unit) {
    val colors = MyTheme.colors

    Column(
        modifier
            .fillMaxWidth()
            .background(colors.bgContent, BlockShape)
            .border(Tokens.Size.Hairline, colors.border, BlockShape),
    ) {
        content()
    }
}

@Composable
private fun CardHead(content: @Composable RowScope.() -> Unit) {
    val colors = MyTheme.colors

    Column(Modifier.fillMaxWidth().background(colors.bgPanel)) {
        Row(
            Modifier.fillMaxWidth().height(Tokens.Size.DiffFile).padding(horizontal = Tokens.Space.S2),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            content()
        }
        Hairline()
    }
}

@Composable
private fun DiffCard(diff: FileDiff, status: ToolStatus) {
    val colors = MyTheme.colors

    Card(Modifier.semantics { contentDescription = "Edit to ${diff.path}, ${diff.added} added, ${diff.removed} removed" }) {
        CardHead {
            Icon(MyIcon.File, colors.textMuted, Tokens.Size.IconSm)
            Label(diff.path, Modifier.weight(1f), Tokens.Type.Code, maxLines = 1)
            DiffStat(diff)

            if (status == ToolStatus.Failed) {
                StatusMark("Failed", Tone.Failed, MyIcon.Failed)
            }
        }

        Column(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState())) {
            for (line in diff.lines) {
                DiffRow(line)
            }
        }
    }
}

@Composable
private fun DiffRow(line: DiffLine) {
    val colors = MyTheme.colors
    val background = when (line.kind) {
        DiffLine.Kind.Added -> colors.diffAddBg
        DiffLine.Kind.Removed -> colors.diffDelBg
        DiffLine.Kind.Context -> Color.Transparent
    }
    val (sign, signColor) = when (line.kind) {
        DiffLine.Kind.Added -> "+" to colors.diffAdd
        DiffLine.Kind.Removed -> "-" to colors.diffDel
        DiffLine.Kind.Context -> "" to colors.textFaint
    }

    Row(Modifier.background(background)) {
        Label(line.oldNumber?.toString() ?: "", Modifier.width(Tokens.Size.DiffNumber).padding(end = Tokens.Space.S2), Tokens.Type.CodeSm.copy(textAlign = TextAlign.End), colors.textFaint)
        Label(line.newNumber?.toString() ?: "", Modifier.width(Tokens.Size.DiffNumber).padding(end = Tokens.Space.S2), Tokens.Type.CodeSm.copy(textAlign = TextAlign.End), colors.textFaint)
        Label(sign, Modifier.width(Tokens.Size.DiffSign), Tokens.Type.Code, signColor)
        Label(line.text, style = Tokens.Type.Code)
    }
}

@Composable
private fun PlanCard(plan: ThreadItem.Plan) {
    val colors = MyTheme.colors

    Card {
        CardHead { Label("Plan", style = Tokens.Type.UiStrong) }

        Column(Modifier.padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            for (entry in plan.entries) {
                val (icon, tint, word) = when (entry.status) {
                    PlanStatus.Done -> Triple(MyIcon.Check, colors.textMuted, "Done")
                    PlanStatus.Running -> Triple(MyIcon.Working, colors.statusWorking, "In progress")
                    PlanStatus.Pending -> Triple(MyIcon.Clock, colors.textMuted, "Waiting")
                }

                Row(
                    Modifier.semantics(mergeDescendants = true) { contentDescription = "$word: ${entry.content}" },
                    horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Icon(icon, tint, Tokens.Size.IconSm)
                    Label(entry.content, style = Tokens.Type.Ui, color = if (entry.status == PlanStatus.Done) colors.textMuted else colors.text)
                }
            }
        }
    }
}

@Composable
private fun PermissionCard(permission: ThreadItem.Permission, onAnswer: (String, String) -> Unit) {
    Card {
        CardHead {
            StatusMark(if (permission.resolved) "Answered" else "Needs your answer", if (permission.resolved) Tone.Muted else Tone.Attention, if (permission.resolved) MyIcon.Check else MyIcon.Alert)
        }

        Box(Modifier.padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2)) {
            Label(permission.title, style = Tokens.Type.Code)
        }

        if (!permission.resolved) {
            Hairline()

            Row(
                Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1_5),
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End),
            ) {
                for (option in permission.options) {
                    val allows = option.kind.startsWith("allow")

                    MyButton(
                        onClick = { onAnswer(permission.requestId, option.id) },
                        text = option.name,
                        variant = if (allows) ButtonVariant.Primary else ButtonVariant.Outline,
                        size = ButtonSize.Md,
                    )
                }
            }
        }
    }
}

@Composable
private fun TurnEnd(item: ThreadItem.TurnEnd, agentLine: String) {
    val text = when (item.stopReason) {
        "end_turn" -> agentLine
        "cancelled" -> "Stopped"
        else -> "Stopped: ${item.stopReason.replace('_', ' ')}"
    }

    Label(text, style = Tokens.Type.Meta, color = MyTheme.colors.textMuted)
}

@Composable
private fun WorkflowCard(progress: WorkflowProgress, onStop: () -> Unit) {
    val colors = MyTheme.colors

    Card {
        CardHead {
            if (progress.complete) {
                Icon(MyIcon.Check, colors.textMuted, Tokens.Size.IconSm)
            } else {
                Icon(MyIcon.Working, colors.statusWorking, Tokens.Size.IconSm)
            }

            Label("Workflow", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
            Label("${progress.finishedCount} of ${progress.agents.size} agents", Modifier.weight(1f), Tokens.Type.Meta, colors.textMuted, maxLines = 1)

            if (!progress.complete) {
                MyButton(onStop, text = "Stop", icon = MyIcon.Stop, description = "Stop the workflow", size = ButtonSize.Sm, muted = true)
            }
        }

        Row(
            Modifier
                .fillMaxWidth()
                .padding(start = Tokens.Space.S3, top = Tokens.Space.S2, end = Tokens.Space.S3)
                .semantics { contentDescription = "${progress.finishedCount} of ${progress.agents.size} agents finished" },
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5),
        ) {
            for (agent in progress.agents) {
                Box(
                    Modifier
                        .weight(1f)
                        .height(Tokens.Size.ProgressBar)
                        .background(if (agent.finished) colors.accent else colors.statusWorking, RoundedCornerShape(Tokens.Radius.Full)),
                )
            }
        }

        for (phase in progress.phases) {
            val inPhase = progress.agents.filter { it.phase == phase }

            Column(Modifier.padding(top = Tokens.Space.S1_5, bottom = Tokens.Space.S0_5)) {
                Row(Modifier.padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S1), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                    Label(phase, Modifier.semantics { heading() }, Tokens.Type.MetaStrong)
                    Label("${inPhase.count { it.finished }} of ${inPhase.size}", style = Tokens.Type.Meta, color = colors.textMuted)
                }

                for (agent in inPhase) {
                    Row(
                        Modifier
                            .fillMaxWidth()
                            .padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S0_5)
                            .semantics(mergeDescendants = true) { contentDescription = "${if (agent.finished) "Done" else "Running"}: ${agent.label}" },
                        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        if (agent.finished) {
                            Icon(MyIcon.Check, colors.textMuted, Tokens.Size.IconSm)
                        } else {
                            Icon(MyIcon.Working, colors.statusWorking, Tokens.Size.IconSm)
                        }

                        Label(agent.label, Modifier.weight(1f), maxLines = 1)
                    }
                }
            }
        }
    }
}
