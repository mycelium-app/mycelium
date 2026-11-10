package mycelium.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.border
import androidx.compose.ui.draw.clip
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import kotlin.time.Clock
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import mycelium.api.Project
import mycelium.state.AppState
import mycelium.state.Store
import mycelium.state.parsedMarkdown
import mycelium.state.ThreadItem
import mycelium.api.ConversationStatus
import mycelium.state.agentAccounts
import mycelium.state.relativeTime
import mycelium.state.compactWindowUsage
import mycelium.state.formatCount
import mycelium.state.formatTokens
import mycelium.state.windowName

private val SparklineHeight = 48.dp
private val WideWidth = 1100.dp
private val MediumWidth = 700.dp
private val StatsWidth = 260.dp
private val FeedWidth = 320.dp
private val GaugeWidth = 120.dp
private val GaugeStroke = 8.dp
private val FocusOutline = 2.dp
private const val PollMillis = 15_000L
private const val ResolvedLingerMillis = 4_000L
private const val RecentMessages = 3

private const val NeedsYouTab = "Needs you"
private const val FeedTab = "Feed"
private const val StatsTab = "Stats"

data class FeedPost(
    val author: String,
    val avatarLetter: String,
    val time: String,
    val text: String,
    val conversationId: String,
    val projectName: String?,
    val conversationTitle: String,
    val project: Project?,
)

private data class InboxStats(val rate: TokenRate?, val now: kotlin.time.Instant)

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun InboxScreen(store: Store, state: AppState, onOpenConversation: (String) -> Unit, onOpenUsageHistory: () -> Unit) {
    val items = rememberInbox(store, state)
    val lingering = remember { mutableStateMapOf<String, InboxItem>() }
    var previous by remember { mutableStateOf(items) }
    var stats by remember { mutableStateOf(InboxStats(null, Clock.System.now())) }
    var showingStats by remember { mutableStateOf(false) }
    var tab by remember { mutableStateOf(NeedsYouTab) }
    val scope = rememberCoroutineScope()
    val voice by store.voice.state.collectAsState()
    val posts = state.feedPosts.map { post ->
        val project = state.conversations[post.conversationId]?.projectId?.let { state.projects[it] }

        FeedPost(post.author, post.author.take(1).uppercase(), relativeTime(post.createdAt), post.message, post.conversationId, post.project, post.conversationTitle, project)
    }

    LaunchedEffect(items) {
        val present = items.map { it.key }.toSet()

        for (gone in previous.filter { it.key !in present }) {
            val kept = gone.resolved() ?: continue

            lingering[gone.key] = kept
            scope.launch {
                delay(ResolvedLingerMillis)
                lingering.remove(gone.key)
            }
        }

        lingering.keys.removeAll(present)
        previous = items
    }

    LaunchedEffect(store, state.connected) {
        store.refreshFeed()
    }

    LaunchedEffect(voice.open) {
        if (voice.open) {
            tab = NeedsYouTab
        }
    }

    LaunchedEffect(store) {
        var earlier: TokenSample? = null
        var peak = 0L

        while (true) {
            attempt {
                val activity = store.api.activity("today")
                val sample = tokenSample(activity, Clock.System.now())
                val computed = tokenRate(earlier, sample, activity, peak)

                earlier = sample
                peak = computed.peak
                stats = InboxStats(computed, sample.at)
            }

            delay(PollMillis)
        }
    }

    val shown = (items + lingering.values.filter { lingered -> items.none { it.key == lingered.key } }).sortedByDescending { it.waitingSince }
    val needsYou: @Composable (Modifier, Boolean) -> Unit = { modifier, headed -> NeedsYouColumn(store, state, shown, state.needsYouFocus, voice.open, onOpenConversation, modifier, headed) }
    val feedColumn: @Composable (Modifier) -> Unit = { modifier -> FeedColumn(posts, onOpenConversation, modifier) }
    val statsColumn: @Composable (Modifier) -> Unit = { modifier -> StatsColumn(store, state, stats, onOpenConversation, onOpenUsageHistory, modifier) }

    Column(Modifier.fillMaxSize()) {
        Bar {
            OpenListButton()
            BarTitle("Inbox")
        }

        BoxWithConstraints(Modifier.weight(1f).fillMaxWidth()) {
            val halfHeight = maxHeight / 2

            if (maxWidth >= WideWidth) {
                Row(Modifier.fillMaxSize()) {
                    statsColumn(Modifier.width(StatsWidth).fillMaxHeight())
                    VerticalHairline()
                    needsYou(Modifier.weight(1f).fillMaxHeight(), true)
                    VerticalHairline()
                    feedColumn(Modifier.width(FeedWidth).fillMaxHeight())
                }
            } else if (maxWidth >= MediumWidth) {
                Column(Modifier.fillMaxSize()) {
                    StatsStrip(state, stats, showingStats, onToggle = { showingStats = !showingStats })
                    Hairline()

                    if (showingStats) {
                        statsColumn(Modifier.fillMaxWidth().heightIn(max = halfHeight))
                        Hairline()
                    }

                    Row(Modifier.weight(1f).fillMaxWidth()) {
                        needsYou(Modifier.weight(1f).fillMaxHeight(), true)
                        VerticalHairline()
                        feedColumn(Modifier.width(FeedWidth).fillMaxHeight())
                    }
                }
            } else {
                Column(Modifier.fillMaxSize()) {
                    TabRow(listOf(NeedsYouTab, FeedTab, StatsTab), tab, { tab = it }, groupLabel = "Inbox sections", modifier = Modifier.padding(horizontal = Tokens.Space.S2))
                    Hairline()

                    when (tab) {
                        NeedsYouTab -> needsYou(Modifier.weight(1f).fillMaxWidth(), false)
                        FeedTab -> feedColumn(Modifier.weight(1f).fillMaxWidth())
                        else -> statsColumn(Modifier.weight(1f).fillMaxWidth())
                    }
                }
            }
        }
    }
}

@Composable
private fun ColumnHeading(text: String) {
    Label(text, Modifier.padding(horizontal = Tokens.Space.S4, vertical = Tokens.Space.S3).semantics { heading() }, Tokens.Type.UiStrong, MyTheme.colors.textMuted)
}

@Composable
private fun NeedsYouColumn(store: Store, state: AppState, items: List<InboxItem>, focusedKey: String?, voiceInUse: Boolean, onOpenConversation: (String) -> Unit, modifier: Modifier, headed: Boolean) {
    val listState = rememberLazyListState()
    val outlinedKey = focusedKey?.takeIf { key -> items.any { it.key == key } } ?: items.firstOrNull()?.key?.takeIf { voiceInUse }

    LaunchedEffect(outlinedKey, items) {
        val index = items.indexOfFirst { it.key == outlinedKey }

        if (index >= 0) {
            listState.scrollToItem(index)
        }
    }

    Column(modifier) {
        if (headed) {
            ColumnHeading("Needs you")
        }

        Box(Modifier.weight(1f).fillMaxWidth()) {
            if (items.isEmpty()) {
                Box(Modifier.fillMaxSize().padding(Tokens.Space.S4), contentAlignment = Alignment.Center) {
                    Label("Nothing needs you", style = Tokens.Type.Title, color = MyTheme.colors.textMuted, maxLines = 1)
                }
            } else {
                LazyColumn(Modifier.fillMaxSize(), listState) {
                    items(items, key = { it.key }) { item ->
                        Hairline()
                        InboxItemRow(store, state, item, item.key == outlinedKey, onOpenConversation)
                    }
                }
            }
        }

        Hairline()
        VoiceDock(store, LocalPlatform.current, onOpenConversation)
    }
}

@Composable
private fun InboxItemRow(store: Store, state: AppState, item: InboxItem, focused: Boolean, onOpenConversation: (String) -> Unit) {
    val colors = MyTheme.colors
    var expanded by remember { mutableStateOf(false) }
    val conversation = item.conversation
    val recent = item.thread.filter { it is ThreadItem.UserMessage || it is ThreadItem.AgentMessage }.takeLast(RecentMessages)

    val outline = if (focused) Modifier.border(FocusOutline, colors.accent, BlockShape) else Modifier

    Column(Modifier.fillMaxWidth().then(outline).semantics { selected = focused }.padding(horizontal = Tokens.Space.S4, vertical = Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
            if (item.assistant != null) {
                AssistantAvatar(item.assistant)
            } else {
                ProjectIcon(item.project)
            }

            Row(
                Modifier.weight(1f).pressable({ onOpenConversation(conversation.id) }, description = "Open ${conversation.title}", shape = ControlShape),
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Label(item.projectName, style = Tokens.Type.UiStrong, maxLines = 1)
                Label(conversation.title, Modifier.weight(1f), maxLines = 1)
            }

            Label(relativeTime(item.waitingSince), style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
            MyButton({ expanded = !expanded }, icon = if (expanded) MyIcon.ChevronUp else MyIcon.ChevronDown, description = if (expanded) "Show less of ${conversation.title}" else "Show more of ${conversation.title}", size = ButtonSize.Sm, muted = true)
        }

        item.context?.let { MarkdownText(it, style = Tokens.Type.Ui, color = colors.textMuted, maxLines = 2, overflow = TextOverflow.Ellipsis) }

        if (expanded) {
            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                for (message in recent) {
                    when (message) {
                        is ThreadItem.UserMessage -> MarkdownText("You: ${message.text}", style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 4, overflow = TextOverflow.Ellipsis)
                        is ThreadItem.AgentMessage -> MarkdownText(message.text, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 4, overflow = TextOverflow.Ellipsis)
                        else -> Unit
                    }
                }
            }
        }

        RequestCard(store, state, conversation.id, item.request)
    }
}

@Composable
fun RequestCard(store: Store, state: AppState, conversationId: String, request: InboxRequest) {
    when (request) {
        is InboxRequest.Approval -> ToolApprovalCard(request.permission, onAnswer = { requestId, option -> store.act { store.api.answerPermission(conversationId, requestId, option) } })
        is InboxRequest.Mycelium -> MyceliumCard(request.call, request.parsed)
        is InboxRequest.Gate -> {
            val workflow = state.workflows[request.workflow.id] ?: request.workflow
            val workflows = ConversationWorkflows(listOf(workflow), null, onControl = { workflowId, control -> store.act { store.controlWorkflow(workflowId, control) } })

            CompositionLocalProvider(LocalConversationWorkflows provides workflows) {
                MyceliumWorkflowCard(workflow)
            }
        }
        is InboxRequest.Flag -> FlagCard(request.text, onReply = { store.send(conversationId, it) })
        is InboxRequest.ActionItem -> ActionItemCard(request, onDone = { store.completeActionItem(state.conversations.getValue(conversationId), request.item.id) })
    }
}

@Composable
private fun FlagCard(text: String, onReply: (String) -> Unit) {
    val colors = MyTheme.colors
    var reply by remember { mutableStateOf("") }

    fun send() {
        if (reply.isNotBlank()) {
            onReply(reply)
            reply = ""
        }
    }

    Card {
        CardHead {
            StatusMark("Flagged for you", Tone.Attention, MyIcon.Alert)
            Label("Waiting for a reply", Modifier.weight(1f), Tokens.Type.Meta.copy(textAlign = TextAlign.End), colors.textMuted, maxLines = 1)
        }

        Column(Modifier.padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            if (text.isNotBlank()) {
                MarkdownBlocks(parsedMarkdown(text), textStyle = Tokens.Type.Ui, blockSpacing = Tokens.Space.S2)
            }

            TextInput(reply, { reply = it }, "Reply", Modifier.fillMaxWidth(), placeholder = "Reply to this chat", onSubmit = ::send)

            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                MyButton(::send, text = "Send reply", variant = ButtonVariant.Outline)
            }
        }
    }
}

@Composable
private fun ActionItemCard(request: InboxRequest.ActionItem, onDone: () -> Unit) {
    val colors = MyTheme.colors

    Card {
        CardHead {
            StatusMark(if (request.done) "Done" else "For you to do", if (request.done) Tone.Muted else Tone.Attention, if (request.done) MyIcon.Check else MyIcon.Clipboard)

            if (!request.done) {
                Spacer(Modifier.weight(1f))
                Icon(MyIcon.Clock, colors.textMuted, Tokens.Size.IconSm)
                Label("Stays until you mark it done", style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
            }
        }

        Column(Modifier.padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            MarkdownBlocks(parsedMarkdown(request.item.text), textStyle = Tokens.Type.Ui, blockSpacing = Tokens.Space.S2)

            if (request.done) {
                Label("Marked as done. It leaves Needs you.", Modifier.semantics { liveRegion = LiveRegionMode.Polite }, Tokens.Type.Meta, colors.textMuted)
            } else {
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                    MyButton(onDone, text = "Mark as done", variant = ButtonVariant.Primary, description = "Mark as done: ${request.item.text}")
                }
            }
        }
    }
}

@Composable
private fun FeedColumn(posts: List<FeedPost>, onOpenConversation: (String) -> Unit, modifier: Modifier) {
    Column(modifier) {
        ColumnHeading("Feed")

        if (posts.isEmpty()) {
            Label("No posts yet", Modifier.padding(horizontal = Tokens.Space.S4), Tokens.Type.Ui, MyTheme.colors.textMuted)
        } else {
            LazyColumn(Modifier.fillMaxSize(), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
                items(posts) { post -> FeedPostRow(post, onOpenConversation) }
            }
        }
    }
}

@Composable
private fun FeedPostRow(post: FeedPost, onOpenConversation: (String) -> Unit) {
    val colors = MyTheme.colors
    val place = listOfNotNull(post.projectName, post.conversationTitle).joinToString(", ")

    Row(Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S4), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        Box(Modifier.size(Tokens.Size.Avatar).clip(CircleShape).background(colors.bgSelected), contentAlignment = Alignment.Center) {
            Label(post.avatarLetter, style = Tokens.Type.MetaStrong, color = colors.textMuted)
        }

        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5)) {
            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                Label(post.author, Modifier.weight(1f), Tokens.Type.UiStrong, maxLines = 1)
                Label(post.time, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
            }

            MarkdownBlocks(parsedMarkdown(post.text), textStyle = Tokens.Type.Ui, blockSpacing = Tokens.Space.S0_5)

            Row(
                Modifier.heightIn(min = touchMin(Tokens.Size.ControlSm + Tokens.Space.S0_5)).pressable({ onOpenConversation(post.conversationId) }, description = "Open chat: $place", shape = ControlShape),
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                if (post.projectName != null) {
                    ProjectIcon(post.project)
                }

                Label(place, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
            }
        }
    }
}

@Composable
private fun StatsStrip(state: AppState, stats: InboxStats, showingAll: Boolean, onToggle: () -> Unit) {
    val colors = MyTheme.colors
    val platform = LocalPlatform.current
    val windows = agentAccounts(state.nodes.values).firstOrNull { it.windows.isNotEmpty() }?.windows.orEmpty()
    val spans = listOfNotNull(
        stats.rate?.let { "${formatTokens(it.today)} tokens today" },
        *windows.map { window -> "${windowName(window.name)} ${compactWindowUsage(window, { millis -> platform?.localDateTime(millis) }, stats.now).summary}" }.toTypedArray(),
        stats.rate?.let { "${formatCount(it.perSecond)} tok/s now" },
    )

    Row(Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S4, vertical = Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
        FlowRow(Modifier.weight(1f), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S4), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5)) {
            for (span in spans) {
                Label(span, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
            }
        }

        MyButton(onToggle, text = "All stats", trailingIcon = if (showingAll) MyIcon.ChevronUp else MyIcon.ChevronDown, size = ButtonSize.Sm, muted = true, stateDescription = if (showingAll) "Showing" else "Hidden")
    }
}

@Composable
private fun StatsColumn(store: Store, state: AppState, stats: InboxStats, onOpenConversation: (String) -> Unit, onOpenUsageHistory: () -> Unit, modifier: Modifier) {
    val colors = MyTheme.colors
    val rate = stats.rate
    val accounts = agentAccounts(state.nodes.values).filter { it.windows.isNotEmpty() }
    val running = state.conversations.values.filter { it.status == ConversationStatus.Working && it.archivedAt == null }.sortedByDescending { it.updatedAt }

    Column(modifier.verticalScroll(rememberScrollState())) {
        ColumnHeading("Token spend")

        Column(Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S4), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            Gauge(rate?.perSecond ?: 0, rate?.peak ?: 0)
            Label("Peak today ${formatCount(rate?.peak ?: 0)}", style = Tokens.Type.Meta, color = colors.textMuted)
        }

        Column(Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S4, vertical = Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.Bottom) {
                Label(formatTokens(rate?.today ?: 0), style = Tokens.Type.Display)
                Label("today", style = Tokens.Type.Meta, color = colors.textMuted)
            }

            if (rate != null && rate.hourly.isNotEmpty()) {
                BarChart(rate.hourly, "Tokens per hour today", Modifier.height(SparklineHeight))
            }

            Row(Modifier.fillMaxWidth()) {
                Label("Last hour", Modifier.weight(1f), Tokens.Type.Meta)
                Label("${formatTokens(rate?.lastHour ?: 0)} tokens", style = Tokens.Type.Meta, color = colors.textMuted)
            }
        }

        Row(Modifier.fillMaxWidth().padding(start = Tokens.Space.S4, end = Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
            Label("Usage", Modifier.weight(1f).semantics { heading() }, Tokens.Type.UiStrong, colors.textMuted)
            MyButton(onOpenUsageHistory, icon = MyIcon.Move, description = "Usage history", size = ButtonSize.Sm, muted = true)
        }

        if (accounts.isEmpty()) {
            Label("No account has reported usage yet.", Modifier.padding(Tokens.Space.S4), Tokens.Type.Meta, colors.textMuted)
        }

        for (account in accounts) {
            Box(Modifier.padding(horizontal = Tokens.Space.S4 - Tokens.Space.S1_5)) {
                UsageBlock(account, thisChat = false, onUseReset = { target, key -> useBankedReset(store, target, key) })
            }
        }

        ColumnHeading("Running")

        if (running.isEmpty()) {
            Label("Nothing running", Modifier.padding(horizontal = Tokens.Space.S4), Tokens.Type.Ui, colors.textMuted)
        }

        for (conversation in running) {
            val project = conversation.projectId?.let { state.projects[it] }
            val perSecond = rate?.running?.get(conversation.id)

            Row(
                Modifier.fillMaxWidth().pressable({ onOpenConversation(conversation.id) }, description = "Open ${conversation.title}").padding(horizontal = Tokens.Space.S4, vertical = Tokens.Space.S1_5),
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                ProjectIcon(project)
                Label(project?.name ?: mycelium.state.projectName(conversation.cwd), style = Tokens.Type.UiStrong, maxLines = 1)
                Label(conversation.title, Modifier.weight(1f), maxLines = 1)
                perSecond?.let { Label("${formatCount(it)} tok/s", style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1) }
            }
        }
    }
}

@Composable
private fun Gauge(perSecond: Long, peak: Long, width: Dp = GaugeWidth) {
    val colors = MyTheme.colors
    val fraction = if (peak <= 0) 0f else (perSecond.toFloat() / peak).coerceIn(0f, 1f)

    Box(Modifier.width(width).heightIn(min = width / 2 + Tokens.Space.S4), contentAlignment = Alignment.BottomCenter) {
        Canvas(Modifier.width(width).height(width / 2 + GaugeStroke).align(Alignment.TopCenter).clearAndSetSemantics { }) {
            val stroke = GaugeStroke.toPx()
            val diameter = size.width - stroke
            val corner = Offset(stroke / 2f, stroke / 2f)
            val arc = Size(diameter, diameter)

            drawArc(colors.border, 180f, 180f, false, corner, arc, style = Stroke(stroke, cap = StrokeCap.Round))
            drawArc(colors.accent, 180f, 180f * fraction, false, corner, arc, style = Stroke(stroke, cap = StrokeCap.Round))
        }

        Column(Modifier.semantics(mergeDescendants = true) { contentDescription = "${formatCount(perSecond)} tokens per second now" }, horizontalAlignment = Alignment.CenterHorizontally) {
            Label(formatCount(perSecond), style = Tokens.Type.Display, maxLines = 1)
            Label("tok/s now", style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
        }
    }
}
