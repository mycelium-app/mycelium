package mycelium.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.focusable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Popup
import androidx.compose.ui.window.PopupProperties
import mycelium.api.GitBranches
import mycelium.api.GitRef
import mycelium.api.GraphCommit
import mycelium.state.BranchEntry
import mycelium.state.GraphRow
import mycelium.state.ago
import mycelium.state.branchEntries
import mycelium.state.graphDescription
import mycelium.state.graphRows
import mycelium.state.headLane
import mycelium.state.refDescription

private val LaneWidth = 14.dp
private val GraphRowHeight = 48.dp
private val GraphCurrentStroke = 2.5.dp
private val GraphLaneStroke = 1.5.dp
private val GraphDotRadius = 4.dp
private val ChipIconSize = 12.dp
private const val MostLaneColumns = 8
private const val MostShownRefs = 2
private const val LoadMoreMargin = 20

private val DarkLanes = listOf(0xFF5BA4F5, 0xFF4CC38A, 0xFFF0A24B, 0xFFD17BE8, 0xFFEF6F6F, 0xFF3CC4D6).map(::Color)
private val LightLanes = listOf(0xFF1F6FD6, 0xFF1E8A57, 0xFFB45F06, 0xFF9333B8, 0xFFC62F2F, 0xFF0E7C8C).map(::Color)

@Composable
fun GitGraphTab(commits: List<GraphCommit>, selected: String?, onSelect: (String) -> Unit, onOpen: (GraphCommit) -> Unit, hasMore: Boolean, onMore: () -> Unit, modifier: Modifier = Modifier) {
    val colors = MyTheme.colors
    val rows = remember(commits) { graphRows(commits) }
    val head = remember(rows) { headLane(rows) }
    val gutter = remember(rows) { (rows.maxOfOrNull { it.width } ?: 1).coerceAtMost(MostLaneColumns) }
    val state = rememberLazyListState()
    val source = remember { MutableInteractionSource() }
    val focused = source.collectIsFocusedAsState()
    val index = rows.indexOfFirst { it.commit.hash == selected }

    LaunchedEffect(index) {
        if (index >= 0) {
            state.animateScrollToItem(index)
        }
    }

    LaunchedEffect(rows.size, state.firstVisibleItemIndex, hasMore) {
        if (hasMore && rows.isNotEmpty() && state.firstVisibleItemIndex + state.layoutInfo.visibleItemsInfo.size >= rows.size - LoadMoreMargin) {
            onMore()
        }
    }

    if (rows.isEmpty()) {
        Label("There are no commits yet.", modifier.padding(Tokens.Space.S3), color = colors.textMuted)

        return
    }

    LazyColumn(
        state = state,
        modifier = modifier
            .fillMaxWidth()
            .focusRing(focused, inside = true)
            .focusable(interactionSource = source)
            .semantics { contentDescription = "Commit graph. Use the up and down arrow keys to choose a commit and Enter to open it." }
            .onPreviewKeyEvent { event ->
                if (event.type != KeyEventType.KeyDown) {
                    return@onPreviewKeyEvent false
                }

                when (event.key) {
                    Key.DirectionDown -> onSelect(rows[(index + 1).coerceIn(0, rows.lastIndex)].commit.hash)
                    Key.DirectionUp -> onSelect(rows[(index - 1).coerceIn(0, rows.lastIndex)].commit.hash)
                    Key.Enter -> rows.getOrNull(index)?.let { onOpen(it.commit) }
                    else -> return@onPreviewKeyEvent false
                }

                true
            },
    ) {
        items(rows, key = { it.commit.hash }) { row ->
            GraphItem(row, gutter, head, row.commit.hash == selected, onTap = { onSelect(row.commit.hash); onOpen(row.commit) })
        }
    }
}

@Composable
private fun GraphItem(row: GraphRow, gutter: Int, head: Int?, selected: Boolean, onTap: () -> Unit) {
    val colors = MyTheme.colors
    val commit = row.commit
    val description = graphDescription(commit)

    Row(
        Modifier
            .fillMaxWidth()
            .height(GraphRowHeight)
            .background(if (selected) colors.bgSelected else Color.Transparent)
            .pointerInput(commit.hash) { detectTapGestures { onTap() } }
            .semantics(mergeDescendants = true) {
                contentDescription = description
                role = Role.Button
                this.selected = selected
                onClick { onTap(); true }
            },
        verticalAlignment = Alignment.CenterVertically,
    ) {
        LaneCanvas(row, gutter, head, selected)

        Column(Modifier.weight(1f).padding(end = Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5)) {
            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
                for (ref in commit.refs.take(MostShownRefs)) {
                    RefChip(ref)
                }

                if (commit.refs.size > MostShownRefs) {
                    Label("+${commit.refs.size - MostShownRefs}", style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
                }

                Label(commit.subject, Modifier.weight(1f), maxLines = 1)
            }

            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalAlignment = Alignment.CenterVertically) {
                Label(commit.hash.take(ShortHash), style = Tokens.Type.CodeSm, color = colors.textFaint, maxLines = 1)
                Label(commit.author, Modifier.weight(1f, fill = false), Tokens.Type.Meta, colors.textMuted, maxLines = 1)
                Label(ago(commit.date), style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
            }
        }
    }
}

private const val ShortHash = 7

@Composable
private fun LaneCanvas(row: GraphRow, gutter: Int, head: Int?, selected: Boolean) {
    val colors = MyTheme.colors
    val palette = if (colors.bgContent.luminance() < 0.5f) DarkLanes else LightLanes
    val background = if (selected) colors.bgSelected else colors.bgPanel
    val density = LocalDensity.current
    val laneWidth = with(density) { LaneWidth.toPx() }

    Canvas(Modifier.width(LaneWidth * gutter + Tokens.Space.S2).height(GraphRowHeight)) {
        fun x(column: Int) = column.coerceAtMost(MostLaneColumns - 1) * laneWidth + laneWidth / 2 + Tokens.Space.S1.toPx()

        fun line(from: Offset, to: Offset, lane: Int) {
            val width = if (lane == head) GraphCurrentStroke.toPx() else GraphLaneStroke.toPx()

            drawLine(palette[lane % palette.size], from, to, width, StrokeCap.Round)
        }

        val middle = size.height / 2

        row.through.forEach { line(Offset(x(it.from), 0f), Offset(x(it.to), size.height), it.lane) }
        row.above.forEach { line(Offset(x(it.from), 0f), Offset(x(it.to), middle), it.lane) }
        row.below.forEach { line(Offset(x(it.from), middle), Offset(x(it.to), size.height), it.lane) }

        val centre = Offset(x(row.column), middle)
        val radius = GraphDotRadius.toPx()
        val colour = palette[row.lane % palette.size]

        if (row.isMerge) {
            drawCircle(background, radius, centre)
            drawCircle(colour, radius, centre, style = Stroke(GraphCurrentStroke.toPx()))
        } else {
            drawCircle(colour, radius, centre)
        }
    }
}

@Composable
private fun RefChip(ref: GitRef) {
    val colors = MyTheme.colors
    val filled = ref.current || ref.kind == "head"
    val shape = RoundedCornerShape(Tokens.Radius.Full)
    val tint = if (filled) colors.onAccent else colors.textMuted
    val icon = when (ref.kind) {
        "remote" -> MyIcon.Server
        "tag" -> MyIcon.Tag
        else -> MyIcon.Branch
    }

    Row(
        Modifier
            .height(Tokens.Size.ControlSm)
            .background(if (filled) colors.accent else Color.Transparent, shape)
            .border(Tokens.Size.Hairline, if (filled) colors.accent else colors.borderControl, shape)
            .padding(horizontal = Tokens.Space.S1_5)
            .semantics { contentDescription = refDescription(ref) },
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(icon, tint, ChipIconSize)
        Label(ref.name, Modifier.widthIn(max = ChipMaxWidth), Tokens.Type.Meta, tint, maxLines = 1)
    }
}

private val ChipMaxWidth = 96.dp

private sealed interface PickerItem {
    data class Heading(val text: String) : PickerItem

    data class Entry(val entry: BranchEntry, val position: Int) : PickerItem
}

@Composable
fun BranchPicker(branches: GitBranches?, onSwitch: (BranchEntry) -> Unit, onDismiss: () -> Unit) {
    val colors = MyTheme.colors
    var search by remember { mutableStateOf("") }
    var active by remember { mutableStateOf(0) }
    val focus = remember { FocusRequester() }
    val entries = remember(branches, search) { branches?.let { branchEntries(it, search) }.orEmpty() }
    val items = remember(entries) { pickerItems(entries) }
    val gap = with(LocalDensity.current) { Tokens.Space.S1.roundToPx() }
    val chosen = entries.getOrNull(active.coerceIn(0, maxOf(0, entries.lastIndex)))

    LaunchedEffect(Unit) {
        focus.requestFocus()
    }

    Popup(popupPositionProvider = remember(gap) { BelowAnchor(gap) }, onDismissRequest = onDismiss, properties = PopupProperties(focusable = true)) {
        Column(
            Modifier
                .width(PickerWidth)
                .background(colors.bgRaised, FloatShape)
                .border(Tokens.Size.Hairline, colors.border, FloatShape)
                .padding(Tokens.Space.S1)
                .onPreviewKeyEvent { event ->
                    if (event.type != KeyEventType.KeyDown) {
                        return@onPreviewKeyEvent false
                    }

                    when (event.key) {
                        Key.DirectionDown -> active = (active + 1).coerceAtMost(maxOf(0, entries.lastIndex))
                        Key.DirectionUp -> active = (active - 1).coerceAtLeast(0)
                        Key.Enter -> chosen?.let(onSwitch)
                        Key.Escape -> onDismiss()
                        else -> return@onPreviewKeyEvent false
                    }

                    true
                },
        ) {
            LazyColumn(Modifier.heightIn(max = PickerHeight).semantics { contentDescription = "Branches" }) {
                if (branches == null) {
                    item { Label("Loading branches", Modifier.padding(Tokens.Space.S2), color = colors.textMuted) }
                }

                items(items) { item ->
                    when (item) {
                        is PickerItem.Heading -> Label(item.text, Modifier.padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1).semantics { heading() }, Tokens.Type.Meta, colors.textMuted)
                        is PickerItem.Entry -> BranchRow(item.entry, item.position == active, onClick = { onSwitch(item.entry) })
                    }
                }
            }

            TextInput(
                value = search,
                onValueChange = { search = it; active = 0 },
                label = "Search branches",
                modifier = Modifier.focusRequester(focus).padding(top = Tokens.Space.S1),
                placeholder = "Switch or type to create a branch…",
                showLabel = false,
                leadingIcon = MyIcon.Search,
            )
        }
    }
}

private val PickerWidth = 320.dp
private val PickerHeight = 320.dp

private fun pickerItems(entries: List<BranchEntry>): List<PickerItem> {
    val items = mutableListOf<PickerItem>()

    entries.forEachIndexed { position, entry ->
        val heading = when (entry) {
            is BranchEntry.Local -> "Local branches"
            is BranchEntry.Remote -> "Remote branches"
            is BranchEntry.Create -> null
        }
        val previous = entries.getOrNull(position - 1)

        if (heading != null && (previous == null || previous::class != entry::class)) {
            items += PickerItem.Heading(heading)
        }

        items += PickerItem.Entry(entry, position)
    }

    return items
}

@Composable
private fun BranchRow(entry: BranchEntry, active: Boolean, onClick: () -> Unit) {
    val colors = MyTheme.colors
    val name = when (entry) {
        is BranchEntry.Local -> entry.branch.name
        is BranchEntry.Remote -> entry.branch.name
        is BranchEntry.Create -> "Create branch ${entry.name}"
    }
    val detail = when (entry) {
        is BranchEntry.Local -> "${entry.branch.author} · ${ago(entry.branch.date)} · ${entry.branch.subject}"
        is BranchEntry.Remote -> "${entry.branch.author} · ${ago(entry.branch.date)} · ${entry.branch.subject}"
        is BranchEntry.Create -> null
    }
    val current = entry is BranchEntry.Local && entry.current
    val description = when {
        current -> "$name, current branch"
        entry is BranchEntry.Remote -> "$name, remote branch"
        else -> name
    }

    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = rowHeight())
            .pressable(onClick, role = Role.Button, description = description, shape = ControlShape, selected = active, stateDescription = if (current) "Current branch" else null)
            .padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(
            when {
                current -> MyIcon.Check
                entry is BranchEntry.Remote -> MyIcon.Server
                entry is BranchEntry.Create -> MyIcon.Plus
                else -> MyIcon.Branch
            },
            if (current) colors.text else colors.textMuted,
            Tokens.Size.IconSm,
        )

        Column(Modifier.weight(1f)) {
            Label(name, style = Tokens.Type.Ui, maxLines = 1)

            if (detail != null) {
                Label(detail, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
            }
        }
    }
}
