package mycelium.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.text.BasicText
import androidx.compose.foundation.text.selection.DisableSelection
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.isAltPressed
import androidx.compose.ui.input.key.isCtrlPressed
import androidx.compose.ui.input.key.isMetaPressed
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.unit.dp
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.filterNotNull
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch
import mycelium.api.ChangesLayout
import mycelium.api.Conversation
import mycelium.api.RowRange
import mycelium.api.RowsChunk
import mycelium.state.DiffLayout
import mycelium.state.DiffLine
import mycelium.state.DiffSection
import mycelium.state.DiffTab
import mycelium.state.FileDiff
import mycelium.state.RowChunk
import mycelium.state.Store
import mycelium.state.changeSections
import mycelium.state.commitSections
import mycelium.state.diffLine
import mycelium.state.focusStep
import mycelium.state.splitCommitDiff

private data class ChunkKey(val path: String, val index: Int)

internal class RowSource(private val scope: CoroutineScope, private val fetch: suspend (List<RowRange>) -> List<RowsChunk>) {
    private val chunks = mutableStateMapOf<ChunkKey, List<DiffLine>>()
    private val fingerprints = mutableMapOf<String, String>()
    private val inFlight = mutableSetOf<ChunkKey>()
    private var window = emptyList<RowRange>()

    fun at(path: String, row: Int): DiffLine? = chunks[ChunkKey(path, row / RowChunk)]?.getOrNull(row % RowChunk)

    fun sync(sections: List<DiffSection>) {
        val latest = sections.associate { it.path to it.fingerprint }

        for (key in chunks.keys.toList()) {
            if (latest[key.path] != fingerprints[key.path]) {
                chunks.remove(key)
            }
        }

        fingerprints.clear()
        fingerprints.putAll(latest)
        want(window)
    }

    fun want(windows: List<RowRange>) {
        window = windows

        val missing = mutableListOf<ChunkKey>()

        for (range in windows) {
            for (index in range.from / RowChunk..(range.to - 1) / RowChunk) {
                val key = ChunkKey(range.path, index)

                if (range.path in fingerprints && key !in chunks && key !in inFlight) {
                    missing += key
                }
            }
        }

        if (missing.isEmpty()) {
            return
        }

        inFlight += missing

        val ranges = missing.groupBy { it.path }.flatMap { (path, keys) ->
            val merged = mutableListOf<IntRange>()

            for (index in keys.map { it.index }.sorted()) {
                val last = merged.lastOrNull()

                if (last != null && last.last + 1 == index) merged[merged.lastIndex] = last.first..index else merged += index..index
            }

            merged.map { RowRange(path, it.first * RowChunk, (it.last + 1) * RowChunk) }
        }

        scope.launch {
            var stale = false

            try {
                attempt {
                    for (answer in fetch(ranges)) {
                        if (fingerprints[answer.path] != answer.fingerprint) {
                            stale = true
                            continue
                        }

                        answer.rows.map(::diffLine).chunked(RowChunk).forEachIndexed { offset, part ->
                            chunks[ChunkKey(answer.path, answer.from / RowChunk + offset)] = part
                        }
                    }
                }
            } finally {
                inFlight -= missing.toSet()
            }

            if (stale) {
                want(window)
            }
        }
    }
}

@Composable
fun ChangesTabView(store: Store, conversation: Conversation, tab: DiffTab, modifier: Modifier = Modifier, onRowComposed: () -> Unit = {}) {
    val colors = MyTheme.colors
    val id = conversation.id
    val scope = rememberCoroutineScope()
    var layout by remember(id) { mutableStateOf<ChangesLayout?>(null) }
    var loadFailed by remember(id) { mutableStateOf(false) }
    var failure by remember(id) { mutableStateOf<String?>(null) }
    val run = rememberAttempt { failure = it }
    var editing by remember(id) { mutableStateOf<String?>(null) }
    var original by remember(id) { mutableStateOf("") }
    var draft by remember(id) { mutableStateOf("") }
    var saving by remember(id) { mutableStateOf(false) }
    val source = remember(id) { RowSource(scope) { ranges -> store.api.gitChangesRows(id, ranges).ranges } }
    val sections = remember(layout) { layout?.let { changeSections(it.files) }.orEmpty() }

    LaunchedEffect(id) {
        while (true) {
            val problem = attempt {
                val latest = store.api.gitChangesLayout(id)

                if (latest != layout) {
                    layout = latest
                }
            }

            loadFailed = problem != null

            delay(DiskPollMillis)
        }
    }

    LaunchedEffect(sections) {
        source.sync(sections)
    }

    DisposableEffect(id) {
        onDispose { store.viewingChangesFile(id, null) }
    }

    fun stage(path: String, staging: Boolean) {
        run {
            if (staging) store.api.gitStage(id, listOf(path)) else store.api.gitUnstage(id, listOf(path))

            layout = store.api.gitChangesLayout(id)
        }
    }

    Column(modifier.fillMaxWidth()) {
        failure?.let { ErrorBanner(it, onDismiss = { failure = null }) }

        editing?.let { path ->
            Column(Modifier.fillMaxWidth().padding(Tokens.Space.S2)) {
                Label(path, style = Tokens.Type.Code)
                BasicTextField(
                    value = draft,
                    onValueChange = { draft = it },
                    textStyle = Tokens.Type.Code.themed().copy(color = colors.text),
                    cursorBrush = SolidColor(colors.text),
                    modifier = Modifier.fillMaxWidth().height(240.dp).verticalScroll(rememberScrollState()).semantics { contentDescription = "Edit $path" },
                )
                FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                    MyButton({
                        if (!saving) {
                            saving = true
                            run {
                                try {
                                    store.api.gitFile(id, path, draft, original)
                                    layout = store.api.gitChangesLayout(id)
                                    editing = null
                                } finally {
                                    saving = false
                                }
                            }
                        }
                    }, text = if (saving) "Saving" else "Save")
                    MyButton({ editing = null }, text = "Cancel", variant = ButtonVariant.Outline)
                }
            }
        }

        when {
            layout == null && loadFailed -> LoadFailedState("the changes", onRetry = {
                scope.launch {
                    val problem = attempt { layout = store.api.gitChangesLayout(id) }

                    loadFailed = problem != null
                }
            }, modifier = Modifier.padding(Tokens.Space.S3))
            layout == null -> Label("Loading the changes", Modifier.padding(Tokens.Space.S3), color = colors.textMuted)
            sections.isEmpty() -> Label("There are no changes in this folder.", Modifier.padding(Tokens.Space.S3), color = colors.textMuted)
            else -> DiffStack(
                sections = sections,
                rowAt = source::at,
                onWindow = source::want,
                modifier = Modifier.weight(1f),
                onStage = { stage(it, true) },
                onUnstage = { stage(it, false) },
                onEdit = { path ->
                    run {
                        val loaded = store.api.gitFile(id, path)
                        original = loaded.content
                        draft = loaded.content
                        editing = path
                    }
                },
                onHunk = { section, row, action ->
                    run {
                        store.api.gitHunk(id, section.path, row, section.fingerprint, action)
                        layout = store.api.gitChangesLayout(id)
                    }
                },
                reveal = tab.reveal,
                onRevealed = { store.revealedInDiffTab(id, tab.key) },
                onTopFile = { store.viewingChangesFile(id, it) },
                onRowComposed = onRowComposed,
            )
        }
    }
}

@Composable
fun CommitTabView(store: Store, conversation: Conversation, tab: DiffTab, modifier: Modifier = Modifier, onRowComposed: () -> Unit = {}) {
    val colors = MyTheme.colors
    var shown by remember(tab.key) { mutableStateOf<Pair<List<FileDiff>, Boolean>?>(null) }
    var failed by remember(tab.key) { mutableStateOf(false) }
    var attempts by remember(tab.key) { mutableStateOf(0) }

    LaunchedEffect(tab.key, conversation.id, attempts) {
        failed = false

        val problem = attempt {
            val answer = store.api.gitShow(conversation.id, tab.hash.orEmpty())

            shown = splitCommitDiff(answer.diff) to answer.truncated
        }

        failed = problem != null
    }

    val loaded = shown
    val sections = remember(loaded) { loaded?.let { commitSections(it.first, it.second) }.orEmpty() }
    val lines = remember(loaded) { loaded?.first?.associate { it.path to it.lines }.orEmpty() }

    Column(modifier.fillMaxWidth()) {
        when {
            failed -> LoadFailedState("this diff", onRetry = { attempts += 1 }, modifier = Modifier.padding(Tokens.Space.S3))
            loaded == null -> Label("Loading the diff", Modifier.padding(Tokens.Space.S3), color = colors.textMuted)
            sections.isEmpty() -> Label("There are no text changes to show.", Modifier.padding(Tokens.Space.S3), color = colors.textMuted)
            else -> DiffStack(sections, rowAt = { path, row -> lines[path]?.getOrNull(row) }, onWindow = {}, modifier = Modifier.weight(1f), onRowComposed = onRowComposed)
        }
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
fun DiffStack(
    sections: List<DiffSection>,
    rowAt: (String, Int) -> DiffLine?,
    onWindow: (List<RowRange>) -> Unit,
    modifier: Modifier = Modifier,
    onStage: (String) -> Unit = {},
    onUnstage: (String) -> Unit = {},
    onEdit: ((String) -> Unit)? = null,
    onHunk: ((DiffSection, Int, String) -> Unit)? = null,
    reveal: String? = null,
    onRevealed: () -> Unit = {},
    onTopFile: (String?) -> Unit = {},
    onRowComposed: () -> Unit = {},
) {
    val colors = MyTheme.colors
    val scope = rememberCoroutineScope()
    val list = rememberLazyListState()
    val focusRequester = remember { FocusRequester() }
    var collapsed by remember { mutableStateOf(emptySet<String>()) }
    var expanded by remember { mutableStateOf(emptySet<String>()) }
    var focused by remember { mutableStateOf<String?>(null) }
    val layout = remember(sections, collapsed, expanded) { DiffLayout(sections, collapsed, expanded) }
    val hunkSets = remember(sections) { sections.map { it.hunks.toHashSet() } }
    val stickyPx = with(LocalDensity.current) { (rowHeight() + Tokens.Size.Hairline).roundToPx() }
    val windowing by rememberUpdatedState(onWindow)
    val topFile by rememberUpdatedState(onTopFile)
    val revealed by rememberUpdatedState(onRevealed)

    fun publishWindow(first: Int, last: Int) {
        windowing(layout.windows(first, last, (last - first + 1).coerceAtLeast(1)))
    }

    LaunchedEffect(layout) {
        snapshotFlow { list.layoutInfo.visibleItemsInfo.let { if (it.isEmpty()) null else it.first().index to it.last().index } }
            .filterNotNull()
            .collect { (first, last) -> publishWindow(first, last) }
    }

    LaunchedEffect(layout) {
        snapshotFlow { list.firstVisibleItemIndex }
            .map { layout.sections[layout.sectionAt(it.coerceAtMost(layout.size - 1))].path }
            .distinctUntilChanged()
            .collect { topFile(it) }
    }

    val revealTarget = if (reveal == null) -1 else layout.headerIndex(reveal)

    LaunchedEffect(reveal, revealTarget >= 0) {
        if (reveal != null && revealTarget >= 0) {
            list.scrollToItem(revealTarget)
            focused = "h:$reveal"
            publishWindow(revealTarget, revealTarget + list.layoutInfo.visibleItemsInfo.size.coerceAtLeast(MinimumWindow))
            revealed()
        }
    }

    LaunchedEffect(Unit) {
        try {
            focusRequester.requestFocus()
        } catch (_: IllegalStateException) {
        }
    }

    fun toggle(path: String) {
        if (path !in collapsed) {
            focused = "h:$path"
        }

        collapsed = if (path in collapsed) collapsed - path else collapsed + path
    }

    fun toggleFocused(): Boolean {
        val key = focused ?: return false
        val index = layout.keyIndex(key)

        if (index < 0) return false

        toggle(layout.sections[layout.sectionAt(index)].path)

        return true
    }

    fun moveTo(key: String) {
        focused = key

        val index = layout.keyIndex(key)
        val info = list.layoutInfo
        val visible = info.visibleItemsInfo.firstOrNull { it.index == index }
        val header = key.startsWith("h:")
        val inView = visible != null && visible.offset >= (if (header) 0 else stickyPx) && visible.offset + visible.size <= info.viewportEndOffset

        if (!inView) {
            scope.launch { list.scrollToItem(index, if (header) 0 else -stickyPx) }
        }
    }

    fun step(direction: Int, byFile: Boolean): Boolean {
        focusStep(layout, focused, direction, byFile)?.let(::moveTo)

        return true
    }

    Box(
        modifier
            .onKeyEvent { event ->
                val plain = event.type == KeyEventType.KeyDown && !event.isCtrlPressed && !event.isMetaPressed && !event.isAltPressed

                when {
                    !plain -> false
                    event.key == Key.J || event.key == Key.DirectionDown -> step(1, false)
                    event.key == Key.K || event.key == Key.DirectionUp -> step(-1, false)
                    event.key == Key.N -> step(1, true)
                    event.key == Key.P -> step(-1, true)
                    event.key == Key.Enter || event.key == Key.NumPadEnter -> toggleFocused()
                    else -> false
                }
            }
            .focusRequester(focusRequester)
            .focusable(),
    ) {
        LazyColumn(Modifier.fillMaxSize().scrollbar(list), list) {
            for ((sectionIndex, section) in layout.sections.withIndex()) {
                val language = section.path.substringAfterLast('.', "")
                val hunks = hunkSets[sectionIndex]

                stickyHeader(key = "h:${section.path}", contentType = "file") {
                    FileHeader(section, section.path in collapsed, focused == "h:${section.path}", { toggle(section.path) }, { onStage(section.path) }, { onUnstage(section.path) }, onEdit?.let { { it(section.path) } }, onRowComposed)
                }

                items(
                    count = layout.shown[sectionIndex],
                    key = { "r:${section.path}:$it" },
                    contentType = { if (it in hunks) "hunk" else "line" },
                ) { row ->
                    remember { onRowComposed(); 0 }
                    val line = rowAt(section.path, row)

                    if (row in hunks && onHunk != null && !section.truncated) {
                        Row(Modifier.fillMaxWidth().height(rowHeight()).background(colors.bgPanel), verticalAlignment = Alignment.CenterVertically) {
                            if (line != null) {
                                Label(line.text, Modifier.weight(1f), style = Tokens.Type.Code, color = colors.textMuted, maxLines = 1)
                                val staged = row < section.stagedRows
                                MyButton({ onHunk(section, row, if (staged) "unstage" else "stage") }, text = if (staged) "Unstage hunk" else "Stage hunk", size = ButtonSize.Sm)

                                if (!staged && section.tag != "Untracked") {
                                    MyButton({ onHunk(section, row, "revert") }, text = "Revert hunk", size = ButtonSize.Sm)
                                }
                            }
                        }
                    } else {
                        DiffBodyRow(line, language, row in hunks, focused == "k:$row:${section.path}")
                    }
                }

                if (layout.hiddenRows[sectionIndex] > 0) {
                    item(key = "m:${section.path}", contentType = "more") {
                        Row(Modifier.fillMaxWidth().padding(Tokens.Space.S2)) {
                            MyButton({ expanded = expanded + section.path }, text = "Show the remaining ${layout.hiddenRows[sectionIndex]} lines", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
                        }
                    }
                }

                if (layout.empty[sectionIndex]) {
                    item(key = "e:${section.path}", contentType = "note") {
                        Label("There are no text changes to show.", Modifier.padding(Tokens.Space.S3), color = colors.textMuted)
                    }
                }

                if (layout.cut[sectionIndex]) {
                    item(key = "c:${section.path}", contentType = "note") {
                        Label("The diff is cut at 8 MB.", Modifier.padding(Tokens.Space.S3), style = Tokens.Type.Meta, color = colors.textMuted)
                    }
                }
            }
        }
    }
}

private const val MinimumWindow = 40

@Composable
private fun diffRowHeight(): Dp = with(LocalDensity.current) { Tokens.Type.Code.lineHeight.toDp() }

@Composable
private fun FileHeader(section: DiffSection, collapsed: Boolean, focused: Boolean, onToggle: () -> Unit, onStage: () -> Unit, onUnstage: () -> Unit, onEdit: (() -> Unit)?, onRowComposed: () -> Unit) {
    val colors = MyTheme.colors

    remember { onRowComposed(); 0 }

    Column(Modifier.fillMaxWidth().background(colors.bgPanel).let { if (focused) it.border(Tokens.Size.FocusRing, colors.focus) else it }) {
        Row(
            Modifier.fillMaxWidth().height(rowHeight()).padding(horizontal = Tokens.Space.S2).semantics { heading() },
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Row(
                Modifier.weight(1f).height(rowHeight()).pressable(onToggle, description = if (collapsed) "Expand ${section.path}" else "Collapse ${section.path}", shape = ControlShape).padding(horizontal = Tokens.Space.S1),
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Icon(if (collapsed) MyIcon.ChevronRight else MyIcon.ChevronDown, colors.textMuted, Tokens.Size.IconSm)
                Label(section.path, Modifier.weight(1f), Tokens.Type.Code, maxLines = 1)
            }

            Label("+${section.added}", style = Tokens.Type.Code, color = colors.diffAdd)
            Label("-${section.removed}", style = Tokens.Type.Code, color = colors.diffDel)
            section.tag?.let { Tag(it) }
            OpenInIdeButton(section.path)

            if (onEdit != null) {
                MyButton(onEdit, text = "Edit", description = "Edit file ${section.path}", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
            }

            if (section.canUnstage) {
                MyButton(onUnstage, text = "Unstage", description = "Unstage file ${section.path}", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
            }

            if (section.canStage) {
                MyButton(onStage, text = "Stage", description = "Stage file ${section.path}", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
            }
        }

        Hairline()
    }
}

@Composable
internal fun DiffBodyRow(line: DiffLine?, language: String, hunk: Boolean, focused: Boolean) {
    val colors = MyTheme.colors
    val frame = Modifier.fillMaxWidth().height(diffRowHeight()).let { if (focused) it.border(Tokens.Size.FocusRing, colors.focus) else it }

    if (line == null) {
        Box(frame)

        return
    }

    if (hunk) {
        Row(frame.background(colors.bgPanel).padding(start = Tokens.Size.DiffNumber * 2 + Tokens.Space.S2)) {
            Label(line.text, style = Tokens.Type.Code, color = colors.textMuted, maxLines = 1)
        }

        return
    }

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

    Row(frame.background(background)) {
        DisableSelection {
            Label(line.oldNumber?.toString() ?: "", Modifier.width(Tokens.Size.DiffNumber).padding(end = Tokens.Space.S2), Tokens.Type.CodeSm.copy(textAlign = TextAlign.End), colors.textFaint, maxLines = 1)
            Label(line.newNumber?.toString() ?: "", Modifier.width(Tokens.Size.DiffNumber).padding(end = Tokens.Space.S2), Tokens.Type.CodeSm.copy(textAlign = TextAlign.End), colors.textFaint, maxLines = 1)
            Label(sign, Modifier.width(Tokens.Size.DiffSign), Tokens.Type.Code, signColor, maxLines = 1)
        }

        BasicText(highlighted(language, line.text), style = Tokens.Type.Code.themed(), maxLines = 1, softWrap = false, overflow = TextOverflow.Clip)
    }
}
