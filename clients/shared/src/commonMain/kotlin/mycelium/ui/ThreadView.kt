package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.rememberTextMeasurer
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
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.snapshotFlow
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
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.withStyle
import androidx.compose.foundation.text.BasicText
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.gestures.animateScrollBy
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withLink
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import mycelium.state.MarkdownBlock
import mycelium.state.markdownBlocks
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
    onAnswer: (requestId: String, optionId: String) -> Unit,
    onStopWorkflow: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val state = rememberLazyListState()
    val scope = rememberCoroutineScope()
    val clipboard = LocalClipboardManager.current
    val oldest = items.lastIndex
    val atTop by remember(oldest) {
        derivedStateOf {
            val last = state.layoutInfo.visibleItemsInfo.lastOrNull()

            last == null || (last.index == oldest && last.offset + last.size <= state.layoutInfo.viewportEndOffset)
        }
    }

    val atNewest by remember { derivedStateOf { state.firstVisibleItemIndex == 0 && state.firstVisibleItemScrollOffset == 0 } }
    var following by remember { mutableStateOf(true) }

    LaunchedEffect(state) {
        snapshotFlow { state.isScrollInProgress }.collect { scrolling ->
            if (!scrolling) {
                following = atNewest
            }
        }
    }

    LaunchedEffect(items.size, items.lastOrNull()) {
        if (following) {
            state.scrollToItem(0)
        }
    }

    Box(modifier.fillMaxWidth(), contentAlignment = Alignment.TopCenter) {
        LazyColumn(
            Modifier.widthIn(max = Tokens.Size.Measure).fillMaxWidth().semantics { contentDescription = "Conversation" },
            state = state,
            reverseLayout = true,
            contentPadding = PaddingValues(
                start = Tokens.Space.S4,
                top = Tokens.Space.S8,
                end = Tokens.Space.S4,
                bottom = Tokens.Space.S4,
            ),
            verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4),
        ) {
            itemsIndexed(items.asReversed(), key = { _, item -> item.key }) { reversedIndex, item ->
                val index = items.lastIndex - reversedIndex

                if (item is ThreadItem.TurnEnd) {
                    val asked = items.subList(0, index).indexOfLast { it is ThreadItem.UserMessage }
                    val answer = items.subList(asked + 1, index).filterIsInstance<ThreadItem.AgentMessage>().joinToString("\n\n") { it.text }

                    TurnEnd(
                        item = item,
                        onCopy = if (answer.isEmpty()) null else ({ clipboard.setText(AnnotatedString(answer)) }),
                        onScrollToMessage = if (asked < 0) null else ({
                            scope.launch {
                                val target = items.lastIndex - asked
                                state.animateScrollToItem(target)
                                val placed = state.layoutInfo.visibleItemsInfo.firstOrNull { it.index == target } ?: return@launch
                                state.animateScrollBy(-(state.layoutInfo.viewportEndOffset - placed.offset - placed.size).toFloat())
                            }
                        }),
                        onScrollToTop = if (atTop) null else ({ scope.launch { state.animateScrollToItem(oldest) } }),
                    )
                } else {
                    ThreadRow(item, onAnswer, onStopWorkflow)
                }
            }
        }

        if (!atNewest) {
            MyButton(
                onClick = {
                    following = true
                    scope.launch { state.animateScrollToItem(0) }
                },
                modifier = Modifier.align(Alignment.BottomCenter).padding(bottom = Tokens.Space.S3),
                icon = MyIcon.ScrollToBottom,
                description = "Scroll to the newest message",
                variant = ButtonVariant.Outline,
            )
        }
    }
}

@Composable
private fun ThreadRow(item: ThreadItem, onAnswer: (String, String) -> Unit, onStopWorkflow: () -> Unit) {
    when (item) {
        is ThreadItem.UserMessage -> UserMessage(item.text)
        is ThreadItem.AgentMessage -> AgentMessage(item.text)
        is ThreadItem.ToolCall -> ToolCall(item)
        is ThreadItem.Plan -> PlanCard(item)
        is ThreadItem.Permission -> PermissionCard(item, onAnswer)
        is ThreadItem.TurnEnd -> Unit
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
    val codeFont = LocalFonts.current.mono

    SelectionContainer {
        Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
            for (block in markdownBlocks(text)) {
                when (block) {
                    is MarkdownBlock.Paragraph -> Prose(block.text)
                    is MarkdownBlock.Heading -> BasicText(
                        inlineMarkup(block.text, colors.text, colors.accent, codeFont),
                        Modifier.semantics { heading() },
                        style = (if (block.level <= 2) Tokens.Type.Display else Tokens.Type.Title).themed(),
                    )
                    is MarkdownBlock.Code -> Box(
                        Modifier
                            .fillMaxWidth()
                            .background(colors.bgPanel, BlockShape)
                            .border(Tokens.Size.Hairline, colors.border, BlockShape)
                            .horizontalScroll(rememberScrollState())
                            .padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2),
                    ) {
                        Label(block.text, style = Tokens.Type.Code)
                    }
                    is MarkdownBlock.Bullets -> ListBlock(block.items) {
                        Box(Modifier.height(Tokens.Type.Prose.lineHeight.value.dp), contentAlignment = Alignment.Center) {
                            Dot(colors.textMuted)
                        }
                    }
                    is MarkdownBlock.Numbered -> ListBlock(block.items) { index ->
                        Label("${block.start + index}.", style = Tokens.Type.Prose, color = colors.textMuted)
                    }
                    is MarkdownBlock.Quote -> Row(Modifier.fillMaxWidth().height(IntrinsicSize.Min)) {
                        Box(Modifier.fillMaxHeight().width(Tokens.Size.FocusRing).background(colors.border))
                        Box(Modifier.padding(start = Tokens.Space.S3)) {
                            BasicText(inlineMarkup(block.text, colors.textMuted, colors.accent, codeFont), style = Tokens.Type.Prose.themed())
                        }
                    }
                    MarkdownBlock.Rule -> Hairline()
                    is MarkdownBlock.Table -> TableBlock(block)
                }
            }
        }
    }
}

@Composable
private fun Prose(text: String) {
    val colors = MyTheme.colors

    BasicText(inlineMarkup(text, colors.text, colors.accent, LocalFonts.current.mono), style = Tokens.Type.Prose.themed())
}

@Composable
private fun TableBlock(table: MarkdownBlock.Table) {
    val colors = MyTheme.colors
    val codeFont = LocalFonts.current.mono
    val measurer = rememberTextMeasurer()
    val density = LocalDensity.current
    val rows = listOf(table.header) + table.rows
    val styles = listOf(Tokens.Type.UiStrong.themed()) + List(table.rows.size) { Tokens.Type.Prose.themed() }
    val marked = rows.mapIndexed { index, row -> row.map { inlineMarkup(it, colors.text, colors.accent, codeFont) to styles[index] } }
    val widths = table.header.indices.map { column ->
        val widest = marked.maxOf { row -> measurer.measure(row[column].first, row[column].second).size.width }

        with(density) { minOf(widest.toDp() + Tokens.Space.S3 * 2, Tokens.Size.TableCell) }
    }

    Box(Modifier.fillMaxWidth().border(Tokens.Size.Hairline, colors.border, BlockShape).horizontalScroll(rememberScrollState())) {
        Column {
            marked.forEachIndexed { index, row ->
                if (index > 0) {
                    Hairline(Modifier.width(widths.fold(0.dp) { total, width -> total + width }))
                }

                Row(Modifier.height(IntrinsicSize.Min).background(if (index == 0) colors.bgPanel else Color.Transparent)) {
                    row.forEachIndexed { column, (text, style) ->
                        Box(Modifier.width(widths[column]).fillMaxHeight().padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2)) {
                            BasicText(text, style = style)
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun ListBlock(items: List<String>, marker: @Composable (Int) -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
        items.forEachIndexed { index, item ->
            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                Box(Modifier.widthIn(min = Tokens.Space.S4)) {
                    marker(index)
                }

                Prose(item)
            }
        }
    }
}

private val InlineLink = Regex("\\[([^\\]]+)]\\((https?://[^)\\s]+)\\)")

fun inlineMarkup(text: String, color: Color, codeColor: Color, codeFont: FontFamily? = Tokens.Type.Code.fontFamily): AnnotatedString =
    buildAnnotatedString {
        withStyle(SpanStyle(color = color)) {
            var index = 0

            while (index < text.length) {
                val link = InlineLink.matchAt(text, index)

                if (link != null) {
                    withLink(LinkAnnotation.Url(link.groupValues[2], TextLinkStyles(SpanStyle(color = codeColor, textDecoration = TextDecoration.Underline)))) {
                        append(link.groupValues[1])
                    }
                    index += link.value.length
                    continue
                }

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
                    withStyle(SpanStyle(color = codeColor, fontFamily = codeFont, fontSize = Tokens.Type.Code.fontSize)) { append(inner) }
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

    if (call.kind == "execute") {
        CommandCard(call)
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
private fun CommandCard(call: ThreadItem.ToolCall) {
    val colors = MyTheme.colors
    val output = call.output?.trimEnd().orEmpty()

    Card(Modifier.semantics { contentDescription = "Command ${call.argument}" }) {
        Row(
            Modifier.fillMaxWidth().background(colors.bgPanel).padding(Tokens.Space.S2),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Label(call.argument, Modifier.weight(1f), Tokens.Type.Code)

            when (call.status) {
                ToolStatus.Running -> StatusMark("Running", Tone.Working, MyIcon.Working)
                ToolStatus.Failed -> StatusMark("Failed", Tone.Failed, MyIcon.Failed)
                else -> {}
            }
        }

        if (output.isNotEmpty()) {
            Hairline()
            Box(Modifier.fillMaxWidth().heightIn(max = Tokens.Size.CommandOutput).verticalScroll(rememberScrollState()).padding(Tokens.Space.S2)) {
                Label(output, style = Tokens.Type.CodeSm, color = colors.textMuted)
            }
        }
    }
}

@Composable
internal fun DiffCard(diff: FileDiff, status: ToolStatus) {
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
private fun TurnEnd(item: ThreadItem.TurnEnd, onCopy: (() -> Unit)?, onScrollToMessage: (() -> Unit)?, onScrollToTop: (() -> Unit)?) {
    val stopped = when (item.stopReason) {
        "end_turn" -> null
        "cancelled" -> "Stopped"
        else -> "Stopped: ${item.stopReason.replace('_', ' ')}"
    }

    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5), verticalAlignment = Alignment.CenterVertically) {
        if (stopped != null) {
            Label(stopped, Modifier.padding(end = Tokens.Space.S1), Tokens.Type.Meta, MyTheme.colors.textMuted)
        }

        Box(Modifier.weight(1f))

        if (onCopy != null) {
            MyButton(onCopy, icon = MyIcon.Copy, description = "Copy this agent response", size = ButtonSize.Sm, muted = true)
        }

        if (onScrollToMessage != null) {
            MyButton(onScrollToMessage, icon = MyIcon.ScrollToMessage, description = "Scroll to your message", size = ButtonSize.Sm, muted = true)
        }

        if (onScrollToTop != null) {
            MyButton(onScrollToTop, icon = MyIcon.ScrollToTop, description = "Scroll to top", size = ButtonSize.Sm, muted = true)
        }
    }
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
