package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.foundation.border
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
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
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.style.BaselineShift
import androidx.compose.ui.unit.em
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.withStyle
import androidx.compose.foundation.text.BasicText
import androidx.compose.foundation.text.selection.DisableSelection
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.gestures.animateScrollBy
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import kotlin.time.Clock
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withLink
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import mycelium.state.GeneratedUiLanguage
import mycelium.state.CalloutKind
import mycelium.state.Token
import mycelium.state.ColumnAlign
import mycelium.state.TokenKind
import mycelium.state.highlight
import mycelium.state.chatToolName
import mycelium.state.myceliumCall
import mycelium.state.toolSearchText
import mycelium.state.myceliumTool
import mycelium.state.MarkdownBlock
import mycelium.state.PastedSegment
import mycelium.state.parsePasted
import mycelium.state.MarkdownListItem
import mycelium.state.parseUiBlock
import mycelium.state.pinnedCards
import mycelium.state.clockTime
import mycelium.state.quietFor
import mycelium.state.stalled
import mycelium.state.workflowPinKey
import mycelium.state.parsedMarkdown
import mycelium.state.DiffLine
import mycelium.state.FileDiff
import mycelium.state.PlanStatus
import mycelium.state.ThreadItem
import mycelium.state.ToolStatus
import mycelium.state.UiNode
import mycelium.state.workflowStats
import mycelium.state.workflowTokens
import mycelium.state.suggestedReplies
import mycelium.state.WorkflowProgress
import mycelium.api.Attachment
import mycelium.state.ThreadImage

private const val ShortOutputLimit = 40
private const val OlderLoadThreshold = 5

@Composable
fun ThreadView(
    items: List<ThreadItem>,
    onAnswer: (requestId: String, optionId: String) -> Unit,
    onStopWorkflow: () -> Unit,
    modifier: Modifier = Modifier,
    onSuggest: (String) -> Unit = {},
    onRetry: (() -> Unit)? = null,
    limitActions: LimitActions? = null,
    agent: String = "",
    dismissedPins: Set<String> = emptySet(),
    onUnpin: (String) -> Unit = {},
    hasOlder: Boolean = false,
    loadingOlder: Boolean = false,
    onLoadOlder: () -> Unit = {},
) {
    val state = rememberLazyListState()
    val loadOlder by rememberUpdatedState(onLoadOlder)
    val now = rememberNow(30_000L)
    val pinned = remember(items, dismissedPins, now) { pinnedCards(items, dismissedPins, now) }
    val pinnedKeys = remember(pinned) { pinned.map { it.key }.toSet() }
    val scope = rememberCoroutineScope()
    val clipboard = LocalClipboardManager.current
    val atNewest by remember { derivedStateOf { state.firstVisibleItemIndex == 0 && state.firstVisibleItemScrollOffset == 0 } }
    var following by remember { mutableStateOf(true) }

    LaunchedEffect(state) {
        snapshotFlow { state.isScrollInProgress }.collect { scrolling ->
            if (!scrolling) {
                following = atNewest
            }
        }
    }

    LaunchedEffect(state, hasOlder, loadingOlder) {
        if (hasOlder && !loadingOlder) {
            snapshotFlow { state.layoutInfo.totalItemsCount > 0 && (state.layoutInfo.visibleItemsInfo.lastOrNull()?.index ?: 0) >= state.layoutInfo.totalItemsCount - OlderLoadThreshold }.collect { nearOldest ->
                if (nearOldest) {
                    loadOlder()
                }
            }
        }
    }

    LaunchedEffect(items.size, items.lastOrNull()) {
        if (following) {
            state.requestScrollToItem(0)
        }
    }
    val gallery = remember(items) {
        items.flatMap {
            when (it) {
                is ThreadItem.UserMessage -> it.images
                is ThreadItem.ToolCall -> it.images
                else -> emptyList()
            }
        }.filter { it.sameAs == null }
    }
    val lastFailure = remember(items) { items.indexOfLast { it is ThreadItem.Failure } }


    suspend fun showTopOf(target: Int) {
        state.animateScrollToItem(target)
        val placed = state.layoutInfo.visibleItemsInfo.firstOrNull { it.index == target } ?: return
        state.animateScrollBy(-(state.layoutInfo.viewportEndOffset - placed.offset - placed.size).toFloat())
    }

    val jumpToImage: (String) -> Unit = { key ->
        val target = items.indexOfFirst { it.key == key }

        if (target >= 0) {
            scope.launch { showTopOf(items.lastIndex - target) }
        }
    }

    BoxWithConstraints(modifier.fillMaxWidth(), contentAlignment = Alignment.TopCenter) {
        val height = maxHeight
        val spare = (maxWidth - readingMeasure()) / 2
        val side = maxOf(Tokens.Space.S4, spare + Tokens.Space.S4)
        val beside = spare >= PinnedRailWidth + Tokens.Space.S8

        Column(Modifier.fillMaxSize(), horizontalAlignment = Alignment.CenterHorizontally) {
            Box(Modifier.weight(1f).fillMaxWidth()) {
                LazyColumn(
                    Modifier.fillMaxSize().scrollbar(state, reverse = true).semantics { contentDescription = "Conversation" },
                    state = state,
                    reverseLayout = true,
                    contentPadding = PaddingValues(
                        start = side,
                        top = Tokens.Space.S8,
                        end = side,
                        bottom = Tokens.Space.S4,
                    ),
                    verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4),
                ) {
                    itemsIndexed(items.asReversed(), key = { _, item -> item.key }, contentType = { _, item -> item::class }) { reversedIndex, item ->
                        val index = items.lastIndex - reversedIndex

                        if (item is ThreadItem.TurnEnd) {
                            val asked = items.subList(0, index).indexOfLast { it is ThreadItem.UserMessage }
                            val answer = items.subList(asked + 1, index).filterIsInstance<ThreadItem.AgentMessage>().joinToString("\n\n") { it.text }

                            TurnEnd(
                                item = item,
                                suggestions = if (index == items.lastIndex && item.stopReason == "end_turn") suggestedReplies(answer) else emptyList(),
                                onSuggest = onSuggest,
                                onCopy = if (answer.isEmpty()) null else ({ clipboard.setText(AnnotatedString(answer)) }),
                                onScrollToMessage = if (asked < 0) null else ({ scope.launch { showTopOf(items.lastIndex - asked) } }),
                                onScrollToTop = if (asked + 1 >= index) null else ({ scope.launch { showTopOf(items.lastIndex - (asked + 1)) } }),
                            )
                        } else {
                            CompositionLocalProvider(LocalImageJump provides jumpToImage, LocalImageGallery provides gallery, LocalPinnedKeys provides pinnedKeys) {
                                ThreadRow(item, onAnswer, onStopWorkflow, if (index == lastFailure) onRetry else null, if (index == lastFailure) limitActions else null, agent)
                            }
                        }
                    }

                    if (loadingOlder) {
                        item(key = "loading-older") {
                            Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.Center) {
                                Label("Loading earlier messages", style = Tokens.Type.Meta, color = MyTheme.colors.textMuted)
                            }
                        }
                    }
                }

                if (!atNewest) {
                    Box(Modifier.align(Alignment.BottomEnd).padding(end = if (pinned.isNotEmpty() && beside) PinnedRailWidth + Tokens.Space.S8 else Tokens.Space.S4, bottom = Tokens.Space.S3)) {
                        MyButton(
                            onClick = {
                                following = true
                                scope.launch { state.animateScrollToItem(0) }
                            },
                            icon = MyIcon.ScrollToBottom,
                            description = "Scroll to the newest message",
                            variant = ButtonVariant.Outline,
                        )
                    }
                }
            }

            if (pinned.isNotEmpty() && !beside) {
                PinnedStack(pinned, onStopWorkflow, onUnpin, Modifier.widthIn(max = readingMeasure()).fillMaxWidth().heightIn(max = height / 3))
            }
        }

        if (pinned.isNotEmpty() && beside) {
            PinnedRail(pinned, onStopWorkflow, onUnpin, Modifier.align(Alignment.TopEnd))
        }
    }
}

@Composable
private fun ThreadRow(item: ThreadItem, onAnswer: (String, String) -> Unit, onStopWorkflow: () -> Unit, onRetry: (() -> Unit)?, limitActions: LimitActions?, agent: String) {
    when (item) {
        is ThreadItem.UserMessage -> UserMessage(item.text, item.files, item.images)
        is ThreadItem.AgentMessage -> AgentMessage(item.text)
        is ThreadItem.Thought -> Thought(item)
        is ThreadItem.ToolCall -> Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            if (toolLabel(item.kind, item.title) != "workflow" || item.status == ToolStatus.Failed) {
                ToolCall(item, chat = agent == "chat")
            }

            for (image in item.images) {
                ThreadImageView(image, myceliumTool(item.title)?.label ?: item.title)
            }
        }
        is ThreadItem.Plan -> PlanCard(item)
        is ThreadItem.Permission -> ToolApprovalCard(item, onAnswer)
        is ThreadItem.ResponseEdit -> ResponseEditorCard(item)
        is ThreadItem.TurnEnd -> Unit
        is ThreadItem.Failure -> if (item.limit != null) UsageLimitCard(item.limit, agent, limitActions) else Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
            StatusMark(item.message, Tone.Failed, MyIcon.Failed, Modifier.weight(1f, fill = false).semantics { contentDescription = "Error: ${item.message}" })

            if (onRetry != null) {
                MyButton(onRetry, text = "Retry", size = ButtonSize.Sm, variant = ButtonVariant.Outline, description = "Retry this conversation")
            }
        }
        is ThreadItem.Workflow -> if (workflowPinKey(item.progress.runId) in LocalPinnedKeys.current) PinnedReference("Workflow") else WorkflowCard(item.progress, onStopWorkflow)
        is ThreadItem.ModelChanged -> ModelChangedDivider("Switched to ${item.provider} · ${item.model}")
        is ThreadItem.Continued -> ModelChangedDivider(item.text)
        is ThreadItem.TeachMode -> ModelChangedDivider(if (item.on) "Teach me on" else "Teach me off")
        is ThreadItem.Note -> Label(item.text, style = Tokens.Type.Meta, color = MyTheme.colors.textMuted)
    }
}

@Composable
private fun ModelChangedDivider(text: String) {
    val colors = MyTheme.colors

    Row(Modifier.fillMaxWidth().semantics(mergeDescendants = true) {}, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
        Hairline(Modifier.weight(1f))
        Label(text, style = Tokens.Type.Meta, color = colors.textMuted)
        Hairline(Modifier.weight(1f))
    }
}

@Composable
private fun UserMessage(text: String, files: List<Attachment>, images: List<ThreadImage>) {
    val colors = MyTheme.colors

    Annotatable {
        Column(
            Modifier
                .fillMaxWidth()
                .background(colors.bgPanel, BlockShape)
                .border(Tokens.Size.Hairline, colors.border, BlockShape)
                .padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2),
        ) {
            for (segment in remember(text) { parsePasted(text) }) {
                when (segment) {
                    is PastedSegment.Plain -> if (segment.text.isNotBlank()) {
                        Label(segment.text.trim('\n'), style = Tokens.Type.Prose)
                    }
                    is PastedSegment.Pasted -> PastedMessageBlock(segment)
                }
            }

            if (files.isNotEmpty() || images.isNotEmpty()) {
                MessageAttachments(files, images)
            }
        }
    }
}

@Composable
private fun PastedMessageBlock(pasted: PastedSegment.Pasted) {
    var expanded by remember { mutableStateOf(false) }

    Column(Modifier.padding(vertical = Tokens.Space.S1), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
        Box(Modifier.border(Tokens.Size.Hairline, MyTheme.colors.border, ControlShape)) {
            PastedBadge(pasted.lines, expanded, onClick = { expanded = !expanded })
        }

        if (expanded) {
            Label(pasted.text, style = Tokens.Type.Code)
        }
    }
}

@Composable
private fun Thought(item: ThreadItem.Thought) {
    val colors = MyTheme.colors
    var expanded by remember(item.key, item.finished) { mutableStateOf(!item.finished) }

    Column(Modifier.fillMaxWidth()) {
        MyButton(
            { expanded = !expanded },
            text = if (item.finished) "Thought" else "Thinking",
            icon = if (item.finished) null else MyIcon.Working,
            trailingIcon = if (expanded) MyIcon.ChevronDown else MyIcon.ChevronRight,
            description = if (expanded) "Hide the agent's thinking" else "Show the agent's thinking",
            size = ButtonSize.Sm,
            muted = true,
        )

        if (expanded) {
            Row(Modifier.fillMaxWidth().height(IntrinsicSize.Min).padding(start = Tokens.Space.S2)) {
                Box(Modifier.fillMaxHeight().width(Tokens.Size.FocusRing).background(colors.border))
                Box(Modifier.padding(start = Tokens.Space.S3)) {
                    BasicText(inlineText(item.text.trim(), colors.textMuted), style = Tokens.Type.Prose.themed())
                }
            }
        }
    }
}

@Composable
private fun AgentMessage(text: String) {
    Annotatable {
        MarkdownBlocks(parsedMarkdown(text))
    }
}

@Composable
internal fun MarkdownBlocks(blocks: List<MarkdownBlock>) {
    val colors = MyTheme.colors
    val clipboard = LocalClipboardManager.current

    Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
        for (block in blocks) {
            when (block) {
                is MarkdownBlock.Paragraph -> Prose(block.text)
                is MarkdownBlock.Heading -> BasicText(
                    inlineText(block.text, colors.text),
                    Modifier.semantics { heading() },
                    style = when {
                        block.level <= 2 -> Tokens.Type.Display
                        block.level <= 4 -> Tokens.Type.Title
                        else -> Tokens.Type.UiStrong
                    }.themed(),
                )
                is MarkdownBlock.Code -> {
                    val generated = remember(block.text) { if (block.language == GeneratedUiLanguage) parseUiBlock(block.text) else null }

                    if (generated != null) {
                        if ((generated.sticky || generated.id != null) && generated.pinKey in LocalPinnedKeys.current) {
                            PinnedReference(generated.label)
                        } else {
                            GeneratedUiPanel(generated.node)
                        }
                    } else {
                        Column(Modifier.fillMaxWidth().background(colors.bgPanel, BlockShape).border(Tokens.Size.Hairline, colors.border, BlockShape)) {
                            Row(
                                Modifier.fillMaxWidth().padding(start = Tokens.Space.S3, end = Tokens.Space.S1),
                                horizontalArrangement = Arrangement.SpaceBetween,
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                Label(block.language, style = Tokens.Type.Meta, color = colors.textMuted)
                                MyButton(
                                    { clipboard.setText(AnnotatedString(block.text)) },
                                    icon = MyIcon.Copy,
                                    description = if (block.language.isBlank()) "Copy code" else "Copy ${block.language} code",
                                    size = ButtonSize.Sm,
                                    muted = true,
                                )
                            }

                            Box(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(start = Tokens.Space.S3, end = Tokens.Space.S3, bottom = Tokens.Space.S2)) {
                                BasicText(highlighted(block.language, block.text), style = Tokens.Type.Code.themed())
                            }
                        }

                        if (block.language == GeneratedUiLanguage) {
                            Label("This block could not be shown as an interface.", style = Tokens.Type.Meta, color = colors.textMuted)
                        }
                    }
                }
                is MarkdownBlock.Bullets -> ListBlock(block.items, null)
                is MarkdownBlock.Numbered -> ListBlock(block.items) { index -> "${block.start + index}." }
                is MarkdownBlock.Quote -> Row(Modifier.fillMaxWidth().height(IntrinsicSize.Min)) {
                    Box(Modifier.fillMaxHeight().width(Tokens.Size.FocusRing).background(colors.border))
                    Box(Modifier.weight(1f).padding(start = Tokens.Space.S3)) {
                        MarkdownBlocks(block.blocks)
                    }
                }
                is MarkdownBlock.Callout -> {
                    val tone = calloutTone(block.kind)

                    Row(Modifier.fillMaxWidth().height(IntrinsicSize.Min).background(colors.bgPanel, BlockShape)) {
                        Box(Modifier.fillMaxHeight().width(Tokens.Size.FocusRing).background(tone.color))
                        Column(Modifier.weight(1f).padding(Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                                Icon(tone.icon, tone.color)
                                Label(block.kind.label, style = Tokens.Type.UiStrong, color = tone.color)
                            }

                            MarkdownBlocks(block.blocks)
                        }
                    }
                }
                is MarkdownBlock.Footnotes -> Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                    Hairline()

                    for (note in block.items) {
                        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                            Label(note.label, style = Tokens.Type.Meta, color = colors.textMuted)
                            BasicText(inlineText(note.text, colors.textMuted), style = Tokens.Type.Meta.themed())
                        }
                    }
                }
                MarkdownBlock.Rule -> Hairline()
                is MarkdownBlock.Table -> TableBlock(block)
            }
        }
    }
}

private data class CalloutTone(val color: Color, val icon: MyIcon)

@Composable
private fun calloutTone(kind: CalloutKind): CalloutTone {
    val colors = MyTheme.colors

    return when (kind) {
        CalloutKind.Note -> CalloutTone(colors.statusWorking, MyIcon.Info)
        CalloutKind.Tip -> CalloutTone(colors.diffAdd, MyIcon.Lightbulb)
        CalloutKind.Important -> CalloutTone(colors.accent, MyIcon.Alert)
        CalloutKind.Warning -> CalloutTone(colors.statusAttention, MyIcon.TriangleAlert)
        CalloutKind.Caution -> CalloutTone(colors.statusFailed, MyIcon.Failed)
    }
}
private const val HighlightedCodeLimit = 256

private val highlightedCode = LinkedHashMap<Pair<String, String>, List<Token>>()

private fun highlightedTokens(language: String, code: String): List<Token> {
    val key = language to code
    val tokens = highlightedCode.remove(key) ?: highlight(language, code)
    highlightedCode[key] = tokens

    if (highlightedCode.size > HighlightedCodeLimit) {
        highlightedCode.remove(highlightedCode.keys.first())
    }

    return tokens
}


@Composable
internal fun highlighted(language: String, code: String): AnnotatedString {
    val colors = MyTheme.colors

    return remember(language, code, colors) {
        buildAnnotatedString {
            withStyle(SpanStyle(color = colors.text)) {
                for (token in highlightedTokens(language, code)) {
                    val color = when (token.kind) {
                        TokenKind.Plain -> colors.text
                        TokenKind.Keyword -> colors.statusWorking
                        TokenKind.Text, TokenKind.Added -> colors.diffAdd
                        TokenKind.Comment -> colors.textMuted
                        TokenKind.Number -> colors.statusAttention
                        TokenKind.Removed -> colors.diffDel
                    }

                    withStyle(SpanStyle(color = color)) { append(token.text) }
                }
            }
        }
    }
}

@Composable
private fun inlineText(text: String, color: Color): AnnotatedString {
    val colors = MyTheme.colors
    val platform = LocalPlatform.current

    val mono = LocalFonts.current.mono

    return remember(text, color, colors.accent, mono, platform) { inlineMarkup(text, color, colors.accent, mono, platform?.let { { url -> it.openUrl(url) } }) }
}

@Composable
private fun Prose(text: String) {
    BasicText(inlineText(text, MyTheme.colors.text), style = Tokens.Type.Prose.themed())
}

@Composable
internal fun TableBlock(table: MarkdownBlock.Table) {
    val colors = MyTheme.colors
    val header = table.header.map { inlineText(it, colors.text) }
    val body = table.rows.map { row -> row.map { inlineText(it, colors.text) } }

    val columns = table.header.indices.map { column ->
        val align = table.aligns.getOrElse(column) { ColumnAlign.Start }

        Col("", if (align == ColumnAlign.End) 56.dp else 80.dp, weight = if (align == ColumnAlign.End) 0f else 1f, align = align, overflow = Overflow.Wrap, numeric = align == ColumnAlign.End)
    }

    Table(
        columns,
        body.size,
        { row, column -> TableCell.Rich(body[row][column], Tokens.Type.Prose) },
        fill = false,
        lazy = false,
        headerCell = { column -> TableCell.Rich(header[column], Tokens.Type.UiStrong) },
    )
}

@Composable
private fun ListBlock(items: List<MarkdownListItem>, numbering: ((Int) -> String)?) {
    val colors = MyTheme.colors

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
        items.forEachIndexed { index, item ->
            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                Box(Modifier.widthIn(min = Tokens.Space.S4).height(Tokens.Type.Prose.lineHeight.value.dp), contentAlignment = Alignment.Center) {
                    val checked = item.checked

                    when {
                        checked != null -> Box(
                            Modifier
                                .size(Tokens.Size.IconMd)
                                .semantics { contentDescription = if (checked) "Done" else "Not done" }
                                .background(if (checked) colors.accent else Color.Transparent, BlockShape)
                                .border(Tokens.Size.Hairline, if (checked) colors.accent else colors.textMuted, BlockShape),
                            contentAlignment = Alignment.Center,
                        ) {
                            if (checked) {
                                Icon(MyIcon.Check, colors.onAccent, Tokens.Size.IconSm)
                            }
                        }
                        numbering != null -> Label(numbering(index), style = Tokens.Type.Prose, color = colors.textMuted)
                        else -> Dot(colors.textMuted)
                    }
                }

                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                    Prose(item.text)

                    if (item.children.isNotEmpty()) {
                        MarkdownBlocks(item.children)
                    }
                }
            }
        }
    }
}

private val InlineLink = Regex("\\[([^\\]]+)]\\(([^)\\s]+)(?:\\s+\"[^\"]*\")?\\)")
private val InlineImage = Regex("!\\[([^\\]]*)]\\(([^)\\s]+)(?:\\s+\"[^\"]*\")?\\)")
private val FootnoteReference = Regex("\\[\\^([^\\]\\s]+)]")
private val BareUrl = Regex("<(https?://[^>\\s]+)>|https?://[^\\s<>]+")
private const val UrlTrailing = ".,;:!?)]}'\""

fun inlineMarkup(text: String, color: Color, codeColor: Color, codeFont: FontFamily? = Tokens.Type.Code.fontFamily, open: ((String) -> Unit)? = null): AnnotatedString =
    buildAnnotatedString {
        withStyle(SpanStyle(color = color)) {
            appendInline(text, codeColor, codeFont, open)
        }
    }

private fun AnnotatedString.Builder.appendLink(url: String, codeColor: Color, open: ((String) -> Unit)?, label: AnnotatedString.Builder.() -> Unit) {
    val styles = TextLinkStyles(SpanStyle(color = codeColor, textDecoration = TextDecoration.Underline))
    val annotation = if (open == null) LinkAnnotation.Url(url, styles) else LinkAnnotation.Clickable(url, styles) { open(url) }

    withLink(annotation) { label() }
}

private fun AnnotatedString.Builder.appendInline(text: String, codeColor: Color, codeFont: FontFamily?, open: ((String) -> Unit)?) {
    var index = 0

    while (index < text.length) {
        val previous = if (index == 0) ' ' else text[index - 1]
        val image = InlineImage.matchAt(text, index)
        val link = InlineLink.matchAt(text, index)
        val note = FootnoteReference.matchAt(text, index)
        val bare = if (previous.isLetterOrDigit()) null else BareUrl.matchAt(text, index)

        if (text[index] == '\\' && index + 1 < text.length && !text[index + 1].isLetterOrDigit()) {
            append(text[index + 1])
            index += 2
            continue
        }

        if (note != null) {
            withStyle(SpanStyle(baselineShift = BaselineShift.Superscript, fontSize = 0.75.em, color = codeColor)) { append(note.groupValues[1]) }
            index += note.value.length
            continue
        }

        if (image != null) {
            val url = image.groupValues[2]

            if (image.groupValues[1].isNotBlank()) {
                append(image.groupValues[1] + " ")
            }

            appendLink(url, codeColor, open) { append(url) }
            index += image.value.length
            continue
        }

        if (link != null) {
            appendLink(link.groupValues[2], codeColor, open) { appendInline(link.groupValues[1], codeColor, codeFont, null) }
            index += link.value.length
            continue
        }

        if (bare != null) {
            val wrapped = bare.groupValues[1].isNotEmpty()
            val url = if (wrapped) bare.groupValues[1] else bare.value.trimEnd { it in UrlTrailing }

            appendLink(url, codeColor, open) { append(url) }
            index += if (wrapped) bare.value.length else url.length
            continue
        }

        val marker = listOf("***", "**", "__", "~~", "`", "*", "_", "~", "^").firstOrNull { text.startsWith(it, index) }
        val close = if (marker == null) -1 else closingMarker(text, marker, index, previous)

        if (marker == null || close < 0) {
            append(text[index])
            index++
            continue
        }

        val inner = text.substring(index + marker.length, close)

        when (marker) {
            "`" -> withStyle(SpanStyle(color = codeColor, fontFamily = codeFont, fontSize = Tokens.Type.Code.fontSize)) { append(inner) }
            "~~" -> withStyle(SpanStyle(textDecoration = TextDecoration.LineThrough)) { appendInline(inner, codeColor, codeFont, open) }
            "^" -> withStyle(SpanStyle(baselineShift = BaselineShift.Superscript, fontSize = 0.75.em)) { appendInline(inner, codeColor, codeFont, open) }
            "~" -> withStyle(SpanStyle(baselineShift = BaselineShift.Subscript, fontSize = 0.75.em)) { appendInline(inner, codeColor, codeFont, open) }
            "*", "_" -> withStyle(SpanStyle(fontStyle = FontStyle.Italic)) { appendInline(inner, codeColor, codeFont, open) }
            "***" -> withStyle(SpanStyle(fontWeight = FontWeight.SemiBold, fontStyle = FontStyle.Italic)) { appendInline(inner, codeColor, codeFont, open) }
            else -> withStyle(SpanStyle(fontWeight = FontWeight.SemiBold)) { appendInline(inner, codeColor, codeFont, open) }
        }

        index = close + marker.length
    }
}

private fun closingMarker(text: String, marker: String, index: Int, previous: Char): Int {
    val opens = index + marker.length

    if (marker == "`") {
        return text.indexOf(marker, opens)
    }

    if (opens >= text.length || (marker == "^" || marker == "~") && text.indexOf(marker, opens).let { it < 0 || text.substring(opens, it).any { char -> char.isWhitespace() } }) {
        return -1
    }

    if (opens >= text.length || text[opens].isWhitespace() || (marker.startsWith("_") && previous.isLetterOrDigit())) {
        return -1
    }

    var close = text.indexOf(marker, opens)

    while (close >= 0) {
        val after = text.getOrElse(close + marker.length) { ' ' }
        val single = marker.length == 1
        val clean = !text[close - 1].isWhitespace() && !(single && after == marker[0]) && !(marker.startsWith("_") && after.isLetterOrDigit())

        if (clean && close > opens) {
            return close
        }

        close = text.indexOf(marker, close + (if (single && after == marker[0]) 2 else 1))
    }

    return -1
}

fun toolLabel(kind: String, title: String): String = when (kind) {
    "execute" -> "bash"
    "read", "edit", "search", "fetch", "delete", "move", "think" -> kind
    else -> myceliumTool(title)?.label ?: title.substringBefore(' ').lowercase()
}

@Composable
private fun ToolCall(call: ThreadItem.ToolCall, chat: Boolean) {
    val colors = MyTheme.colors
    val diff = call.diff

    if (diff != null && call.kind == "edit") {
        DiffCard(diff, call.status)
        return
    }

    val mycelium = myceliumCall(call)

    if (mycelium != null) {
        MyceliumCard(call, mycelium)
        return
    }

    val remote = chatToolName(call.title)

    if (remote != null) {
        ChatToolCard(call, remote)
        return
    }

    if (call.kind == "execute") {
        CommandCard(call)
        return
    }

    if (call.kind == "think") {
        val output = call.output?.trim().orEmpty()
        val decision = output.removePrefix("Status: ").takeIf { it in setOf("Approved", "Rejected", "Denied") }
        var expanded by remember(call.key) { mutableStateOf(false) }

        Card(Modifier.semantics { contentDescription = call.title }) {
            CardHead {
                Icon(MyIcon.Review, colors.textMuted, Tokens.Size.IconSm)
                Label(call.title, Modifier.weight(1f), Tokens.Type.UiStrong)

                when {
                    call.status == ToolStatus.Running -> StatusMark("Running", Tone.Working, MyIcon.Working)
                    call.status == ToolStatus.Failed -> StatusMark("Failed", Tone.Failed, MyIcon.Failed)
                    decision != null -> StatusMark(decision, Tone.Muted, if (decision == "Approved") MyIcon.Check else MyIcon.Close)
                }

                if (output.isNotBlank() && decision == null) {
                    MyButton({ expanded = !expanded }, icon = if (expanded) MyIcon.ChevronDown else MyIcon.ChevronRight, description = if (expanded) "Hide ${call.title}" else "Show ${call.title}", size = ButtonSize.Sm, muted = true)
                }
            }

            if (expanded && output.isNotBlank()) {
                Box(Modifier.padding(Tokens.Space.S3)) {
                    MarkdownBlocks(parsedMarkdown(output))
                }
            }
        }

        return
    }

    if (chat) {
        ChatToolCard(call, chatToolName(call.title))
        return
    }

    val toolSearch = toolSearchText(call)

    if (toolSearch != null) {
        Row(
            Modifier.fillMaxWidth().heightIn(min = Tokens.Size.ControlSm),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(if (call.status == ToolStatus.Running) MyIcon.Working else MyIcon.Search, colors.textMuted, Tokens.Size.IconSm)
            Label(toolSearch, Modifier.weight(1f), Tokens.Type.Meta, colors.textMuted, maxLines = 1)

            if (call.status == ToolStatus.Failed) {
                StatusMark("Failed", Tone.Failed, MyIcon.Failed)
            }
        }

        return
    }

    Row(
        Modifier.fillMaxWidth().heightIn(min = Tokens.Size.ControlSm),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Label(toolLabel(call.kind, call.title), style = Tokens.Type.Code, color = colors.text, maxLines = 1)
        Label(call.argument.takeUnless { it == call.title && myceliumTool(it) != null }.orEmpty(), Modifier.weight(1f), Tokens.Type.Code, colors.textMuted, maxLines = 1)

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
internal fun Card(modifier: Modifier = Modifier, content: @Composable () -> Unit) {
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
internal fun CardHead(content: @Composable RowScope.() -> Unit) {
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
            BasicText(highlighted("bash", call.argument), Modifier.weight(1f), style = Tokens.Type.Code.themed())

            when (call.status) {
                ToolStatus.Running -> StatusMark("Running", Tone.Working, MyIcon.Working)
                ToolStatus.Failed -> StatusMark("Failed", Tone.Failed, MyIcon.Failed)
                else -> {}
            }
        }

        if (output.isNotEmpty()) {
            Hairline()
            Box(Modifier.fillMaxWidth().heightIn(max = Tokens.Size.CommandOutput).verticalScrollWithBar().padding(Tokens.Space.S2)) {
                Annotatable {
                    Label(output, style = Tokens.Type.CodeSm, color = colors.textMuted)
                }
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

        Annotatable {
            Column(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState())) {
                for (line in diff.lines) {
                    DiffRow(line)
                }
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
        DisableSelection {
            Label(line.oldNumber?.toString() ?: "", Modifier.width(Tokens.Size.DiffNumber).padding(end = Tokens.Space.S2), Tokens.Type.CodeSm.copy(textAlign = TextAlign.End), colors.textFaint)
            Label(line.newNumber?.toString() ?: "", Modifier.width(Tokens.Size.DiffNumber).padding(end = Tokens.Space.S2), Tokens.Type.CodeSm.copy(textAlign = TextAlign.End), colors.textFaint)
            Label(sign, Modifier.width(Tokens.Size.DiffSign), Tokens.Type.Code, signColor)
        }

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
private fun TurnEnd(item: ThreadItem.TurnEnd, suggestions: List<String>, onSuggest: (String) -> Unit, onCopy: (() -> Unit)?, onScrollToMessage: (() -> Unit)?, onScrollToTop: (() -> Unit)?) {
    val stopped = when (item.stopReason) {
        "end_turn" -> null
        "cancelled" -> "Stopped"
        else -> "Stopped: ${item.stopReason.replace('_', ' ')}"
    }

    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5), verticalAlignment = Alignment.CenterVertically) {
        if (stopped != null) {
            Label(stopped, Modifier.padding(end = Tokens.Space.S1), Tokens.Type.Meta, MyTheme.colors.textMuted)
        }

        Row(Modifier.weight(1f).horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            for (reply in suggestions) {
                MyButton({ onSuggest(reply) }, text = reply, description = "Reply: $reply", size = ButtonSize.Sm, variant = ButtonVariant.Outline)
            }
        }

        if (onCopy != null) {
            MyButton(onCopy, icon = MyIcon.Copy, description = "Copy this agent response", size = ButtonSize.Sm, muted = true)
        }

        if (onScrollToMessage != null) {
            MyButton(onScrollToMessage, icon = MyIcon.ScrollToMessage, description = "Scroll to your message", size = ButtonSize.Sm, muted = true)
        }

        if (onScrollToTop != null) {
            MyButton(onScrollToTop, icon = MyIcon.ScrollToTop, description = "Scroll to the top of this reply", size = ButtonSize.Sm, muted = true)
        }
    }
}

@Composable
internal fun GeneratedUiPanel(node: UiNode) {
    val colors = MyTheme.colors

    Box(Modifier.fillMaxWidth().background(colors.bgPanel, BlockShape).border(Tokens.Size.Hairline, colors.border, BlockShape).padding(Tokens.Space.S3)) {
        GeneratedUi(node)
    }
}

private const val WorkflowTickMillis = 30_000L

@Composable
internal fun WorkflowCard(progress: WorkflowProgress, onStop: () -> Unit, pin: PinControls? = null) {
    val colors = MyTheme.colors
    val platform = LocalPlatform.current
    val now = rememberNow(WorkflowTickMillis, ticking = !progress.over)
    val totals = workflowTokens(progress).takeIf { progress.agents.size > 1 }
    val stalled = progress.stalled(now)
    val quietMinutes = progress.quietFor(now)?.inWholeMinutes ?: 0
    val beforeAnyStep = progress.stopped && progress.agents.isEmpty()
    val outcome = when {
        beforeAnyStep -> "Stopped before any step ran"
        progress.stopped -> "Stopped"
        progress.failed -> "Failed"
        stalled -> "No progress for $quietMinutes minutes"
        else -> null
    }
    val endedAt = progress.end?.endedAt?.let { clockTime(it.toString()) { millis -> platform?.localDateTime(millis) } }
    val explanation = when {
        beforeAnyStep -> if (endedAt != null) "Stopped at $endedAt, before the first agent started." else "Stopped before the first agent started."
        progress.stopped -> if (endedAt != null) "Stopped at $endedAt." else "Stopped."
        progress.failed -> progress.end?.reason
        else -> null
    }

    Card(Modifier.semantics { if (outcome != null) contentDescription = "Workflow ${outcome.replaceFirstChar { it.lowercase() }}" }) {
        CardHead {
            if (progress.complete) {
                Icon(MyIcon.Check, colors.textMuted, Tokens.Size.IconSm)
            } else if (progress.stopped) {
                Icon(MyIcon.Stop, colors.textMuted, Tokens.Size.IconSm)
            } else if (progress.failed) {
                Icon(MyIcon.Failed, colors.statusFailed, Tokens.Size.IconSm)
            } else if (stalled) {
                Icon(MyIcon.Alert, colors.statusAttention, Tokens.Size.IconSm)
            } else {
                Icon(MyIcon.Working, colors.statusWorking, Tokens.Size.IconSm)
            }

            Label("Workflow", Modifier.semantics { heading() }, Tokens.Type.UiStrong)

            if (progress.agents.isNotEmpty()) {
                Label("${progress.finishedCount} of ${progress.agents.size}", if (totals == null && outcome == null) Modifier.weight(1f) else Modifier, Tokens.Type.Meta, colors.textMuted, maxLines = 1)
            }

            if (outcome != null) {
                Label(outcome, Modifier.weight(1f), Tokens.Type.Meta, if (progress.failed) colors.statusFailed else colors.textMuted, maxLines = 1)
            } else if (totals != null) {
                Label(totals, Modifier.weight(1f), Tokens.Type.Meta, colors.textMuted, maxLines = 1)
            }

            if (!progress.over) {
                MyButton(onStop, text = "Stop", icon = MyIcon.Stop, description = "Stop the workflow", size = ButtonSize.Sm, muted = true)
            }

            if (pin != null) {
                PinButtons(pin)
            }
        }

        if (pin?.collapsed == true) {
            return@Card
        }

        if (progress.agents.isNotEmpty()) {
            Row(
                Modifier
                    .fillMaxWidth()
                    .padding(start = Tokens.Space.S3, top = Tokens.Space.S2, end = Tokens.Space.S3)
                    .semantics { contentDescription = "${progress.finishedCount} of ${progress.agents.size} agents finished" },
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5),
            ) {
                for (agent in progress.agents) {
                    val segment = when {
                        agent.failed -> colors.statusFailed
                        agent.finished -> colors.statusDone
                        stalled -> colors.statusAttention
                        progress.ended -> colors.border
                        else -> colors.statusWorking
                    }

                    Box(Modifier.weight(1f).height(Tokens.Size.ProgressBar).background(segment, RoundedCornerShape(Tokens.Radius.Full)))
                }
            }
        }

        if (explanation != null) {
            Label(explanation, Modifier.padding(start = Tokens.Space.S3, top = Tokens.Space.S2, end = Tokens.Space.S3))
        }

        for (phase in progress.phases) {
            val inPhase = progress.agents.filter { it.phase == phase }

            Column(Modifier.padding(top = Tokens.Space.S1_5, bottom = Tokens.Space.S0_5)) {
                Row(Modifier.padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S1), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                    Label(phase, Modifier.semantics { heading() }, Tokens.Type.MetaStrong)
                    Label(if (inPhase.isEmpty()) (if (progress.ended) "Did not run" else "Waiting") else "${inPhase.count { it.finished }} of ${inPhase.size}", style = Tokens.Type.Meta, color = colors.textMuted)
                }

                for (agent in inPhase) {
                    val open = agent.agentId?.let { LocalAgentOpener.current(it) }

                    Row(
                        Modifier
                            .fillMaxWidth()
                            .then(if (open != null) Modifier.pressable(open, shape = BlockShape, description = "Open ${agent.label}") else Modifier)
                            .padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S0_5)
                            .semantics(mergeDescendants = true) { contentDescription = "${if (agent.failed) "Stopped" else if (agent.finished) "Done" else if (progress.ended) "Stopped" else if (stalled) "Stalled" else "Running"}: ${agent.label}" },
                        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        if (agent.failed) {
                            Icon(MyIcon.Failed, colors.statusFailed, Tokens.Size.IconSm)
                        } else if (agent.finished) {
                            Icon(MyIcon.Check, colors.textMuted, Tokens.Size.IconSm)
                        } else if (progress.ended) {
                            Icon(MyIcon.Stop, colors.textMuted, Tokens.Size.IconSm)
                        } else if (stalled) {
                            Icon(MyIcon.Alert, colors.statusAttention, Tokens.Size.IconSm)
                        } else {
                            Icon(MyIcon.Working, colors.statusWorking, Tokens.Size.IconSm)
                        }

                        Label(agent.label, Modifier.weight(1f), maxLines = 1)

                        val stats = if (stalled && !agent.finished) "last line ${quietMinutes}m ago" else workflowStats(agent)

                        stats?.let { Label(it, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1) }
                    }
                }
            }
        }
    }
}
