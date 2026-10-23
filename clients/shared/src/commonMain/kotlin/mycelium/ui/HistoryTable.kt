package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import mycelium.api.Conversation

private val ActionWidth = 48.dp

private val MinColumnWidth = 56.dp

private val MaxColumnWidth = 600.dp

private val DefaultResizeStart = 120.dp

private val TouchColumns = listOf(ColumnSpec(flexible = true), ColumnSpec(min = ActionWidth, max = ActionWidth))

private enum class Naming { Save, Rename }

private fun shownColumns(layout: HistoryLayout): List<HistoryColumn> =
    HistoryColumn.entries.filter { it == HistoryColumn.Title || it in layout.columns }

private fun cellText(entry: HistoryEntry, column: HistoryColumn, stamp: (String, Boolean) -> String): String = when (column) {
    HistoryColumn.Title -> entry.conversation.title
    HistoryColumn.Project -> entry.project
    HistoryColumn.Node -> entry.node
    HistoryColumn.Agent -> entry.agent
    HistoryColumn.Model -> entry.conversation.model.orEmpty()
    HistoryColumn.Started -> stamp(entry.conversation.createdAt, false)
    HistoryColumn.LastActive -> stamp(entry.conversation.updatedAt, false)
    HistoryColumn.Turns -> usageCells(entry.usage)[0]
    HistoryColumn.Tokens -> usageCells(entry.usage)[1]
    HistoryColumn.Cost -> usageCells(entry.usage)[2]
    HistoryColumn.State -> entry.state
}

@Composable
private fun ToolbarMenu(text: String?, icon: MyIcon?, description: String, content: @Composable (close: () -> Unit) -> Unit) {
    var open by remember { mutableStateOf(false) }

    Box {
        MyButton(
            { open = true },
            text = text,
            icon = icon,
            description = description,
            trailingIcon = if (text != null) MyIcon.ChevronDown else null,
            size = ButtonSize.Sm,
            muted = true,
            selected = open,
        )

        if (open) {
            MenuPopup(onDismiss = { open = false }) {
                content { open = false }
            }
        }
    }
}

@Composable
private fun FilterMenu(allLabel: String, description: String, options: List<String>, selected: String?, onSelect: (String?) -> Unit) {
    if (options.size < 2 && selected == null) {
        return
    }

    ToolbarMenu(selected ?: allLabel, null, description) { close ->
        MenuItem(allLabel, onClick = { close(); onSelect(null) }, checked = selected == null)

        for (option in options) {
            MenuItem(option, onClick = { close(); onSelect(option) }, checked = option == selected)
        }
    }
}

@Composable
fun HistoryToolbar(
    layout: HistoryLayout,
    onLayout: (HistoryLayout) -> Unit,
    options: HistoryOptions,
    views: List<HistoryView>,
    onViews: (List<HistoryView>) -> Unit,
    activeView: String?,
    onActiveView: (String?) -> Unit,
) {
    var naming by remember { mutableStateOf<Naming?>(null) }
    val filters = layout.filters

    Bar {
        Row(
            Modifier.weight(1f).horizontalScroll(rememberScrollState()),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Segmented(PastFilter.entries.map { it.label }, filters.state.label, { picked -> onLayout(layout.copy(filters = filters.copy(state = PastFilter.entries.first { it.label == picked }))) }, groupLabel = "State")
            FilterMenu("All projects", "Filter by project", options.projects, filters.project) { onLayout(layout.copy(filters = filters.copy(project = it))) }
            FilterMenu("All nodes", "Filter by node", options.nodes, filters.node) { onLayout(layout.copy(filters = filters.copy(node = it))) }
            FilterMenu("All agents", "Filter by agent", options.agents, filters.agent) { onLayout(layout.copy(filters = filters.copy(agent = it))) }

            ToolbarMenu(layout.grouping.label, null, "Group conversations") { close ->
                for (grouping in HistoryGrouping.entries) {
                    MenuItem(grouping.choice, onClick = { close(); onLayout(layout.copy(grouping = grouping)) }, checked = grouping == layout.grouping)
                }
            }

            ToolbarMenu(null, MyIcon.Sliders, "Choose columns") {
                for (column in HistoryColumn.entries.filter { it != HistoryColumn.Title }) {
                    val shown = column in layout.columns

                    MenuItem(
                        column.title,
                        onClick = { onLayout(layout.copy(columns = if (shown) layout.columns - column else layout.columns + column)) },
                        checked = shown,
                    )
                }
            }

            ToolbarMenu(activeView ?: "Views", null, "Views") { close ->
                for (view in views) {
                    MenuItem(
                        view.name,
                        onClick = {
                            close()
                            onLayout(view.layout)
                            onActiveView(view.name)
                        },
                        checked = view.name == activeView,
                    )
                }

                if (views.isNotEmpty()) {
                    Hairline()
                }

                MenuItem("Save view", onClick = { close(); naming = Naming.Save })

                if (activeView != null) {
                    MenuItem("Rename", onClick = { close(); naming = Naming.Rename })
                    MenuItem(
                        "Delete",
                        onClick = {
                            close()
                            onViews(deleteHistoryView(views, activeView))
                            onActiveView(null)
                        },
                    )
                }
            }
        }
    }

    naming?.let { mode ->
        var name by remember(mode) { mutableStateOf(activeView.orEmpty()) }
        var problem by remember(mode) { mutableStateOf<String?>(null) }
        val confirm = {
            problem = historyViewNameProblem(views, name, activeView)

            if (problem == null) {
                val saved = name.trim()
                onViews(if (mode == Naming.Rename && activeView != null) renameHistoryView(views, activeView, saved) else saveHistoryView(views, saved, layout))
                onActiveView(saved)
                naming = null
            }
        }

        Dialog(onDismiss = { naming = null }) {
            Label(if (mode == Naming.Save) "Name this view" else "Rename view", Modifier.semantics { heading() }, Tokens.Type.Title)
            TextInput(name, { name = it; problem = null }, label = "View name", error = problem, onSubmit = confirm)

            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End)) {
                MyButton({ naming = null }, text = "Cancel", muted = true)
                MyButton(confirm, text = if (mode == Naming.Save) "Save view" else "Rename", variant = ButtonVariant.Primary)
            }
        }
    }
}

@Composable
private fun HeaderCell(column: HistoryColumn, sort: HistorySort, onSort: () -> Unit, onResize: (Dp) -> Unit, onWidth: (Dp) -> Unit) {
    val colors = MyTheme.colors
    val density = LocalDensity.current
    val sorted = sort.column == column
    val ascending = sort.direction == SortDirection.Ascending

    Row(Modifier.fillMaxWidth().height(rowHeight()).onSizeChanged { onWidth(with(density) { it.width.toDp() }) }) {
        Row(
            Modifier
                .weight(1f)
                .fillMaxHeight()
                .pressable(
                    onSort,
                    description = "Sort by ${column.title}",
                    shape = ControlShape,
                    stateDescription = if (!sorted) null else if (ascending) "Sorted ascending" else "Sorted descending",
                ),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Label(column.title, Modifier.weight(1f, fill = false), Tokens.Type.MetaStrong, colors.textMuted, maxLines = 1)

            if (sorted) {
                Icon(if (ascending) MyIcon.ArrowUp else MyIcon.ArrowDown, colors.textMuted, Tokens.Size.IconSm)
            }
        }

        ResizeHandle("Resize ${column.title} column", onResize)
    }
}

@Composable
fun HistoryTable(
    groups: List<HistoryGroup>,
    layout: HistoryLayout,
    onLayout: (HistoryLayout) -> Unit,
    widths: Map<HistoryColumn, Float>,
    onWidths: (Map<HistoryColumn, Float>) -> Unit,
    stamp: (String, Boolean) -> String,
    onOpen: (String) -> Unit,
    modifier: Modifier = Modifier,
    rowMenu: @Composable (Conversation) -> Unit,
) {
    val colors = MyTheme.colors
    val touch = LocalTouch.current
    val measurer = rememberTextMeasurer()
    val density = LocalDensity.current
    val bodyStyle = Tokens.Type.Ui.themed()
    val headerStyle = Tokens.Type.MetaStrong.themed()
    val shown = shownColumns(layout)
    val measured = remember { mutableStateMapOf<HistoryColumn, Dp>() }
    var collapsed by remember(layout.grouping) { mutableStateOf(emptySet<String>()) }
    val flat = groups.flatMap { it.rows }

    val specs = shown.map { column ->
        val stored = widths[column]?.dp

        when {
            column == HistoryColumn.Title -> ColumnSpec(flexible = true, min = stored ?: MinColumnWidth)
            stored != null -> ColumnSpec(min = stored, max = stored)
            else -> ColumnSpec()
        }
    } + ColumnSpec(min = ActionWidth, max = ActionWidth)

    val naturalWidths = remember(flat, shown, widths, touch, bodyStyle, headerStyle) {
        if (touch) {
            listOf(0.dp, ActionWidth)
        } else {
            val widest = { texts: List<String>, style: TextStyle -> with(density) { (texts.maxOfOrNull { measurer.measure(it, style).size.width } ?: 0).toDp() } }
            val sortIcon = Tokens.Size.IconSm + Tokens.Space.S1

            shown.map { column ->
                when {
                    column == HistoryColumn.Title -> 0.dp
                    widths[column] != null -> widths.getValue(column).dp
                    else -> (widest(listOf(column.title), headerStyle) + sortIcon).coerceAtLeast(widest(flat.map { cellText(it, column, stamp) }, bodyStyle))
                }
            } + ActionWidth
        }
    }

    FixedWidthColumns(if (touch) TouchColumns else specs, naturalWidths, modifier) {
        val list = rememberLazyListState()

        LazyColumn(Modifier.fillMaxWidth().scrollbar(list), list) {
            if (!touch) {
                item(key = "columns") {
                    ContentRow(Modifier.fillMaxWidth().semantics { contentDescription = "Columns" }) {
                        for (column in shown) {
                            HeaderCell(
                                column,
                                layout.sort,
                                onSort = { onLayout(layout.copy(sort = layout.sort.after(column))) },
                                onResize = { delta ->
                                    val current = widths[column] ?: (measured[column] ?: DefaultResizeStart).value
                                    onWidths(widths + (column to (current + delta.value).coerceIn(MinColumnWidth.value, MaxColumnWidth.value)))
                                },
                                onWidth = { if (measured[column] != it) measured[column] = it },
                            )
                        }

                        Box(Modifier)
                    }

                    Hairline()
                }
            }

            if (flat.isEmpty()) {
                item(key = "empty") {
                    Label("No past conversations match.", Modifier.padding(Tokens.Space.S4), color = colors.textMuted)
                }
            }

            for (group in groups) {
                val isCollapsed = group.key in collapsed

                if (layout.grouping != HistoryGrouping.None) {
                    item(key = "group:${group.key}") {
                        Row(
                            Modifier
                                .fillMaxWidth()
                                .heightIn(min = rowHeight())
                                .background(colors.bgPanel)
                                .pressable(
                                    { collapsed = if (isCollapsed) collapsed - group.key else collapsed + group.key },
                                    description = "${if (isCollapsed) "Expand" else "Collapse"} ${group.label}",
                                    shape = ControlShape,
                                    stateDescription = if (isCollapsed) "Collapsed" else "Expanded",
                                )
                                .padding(horizontal = Tokens.Space.S3),
                            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Icon(if (isCollapsed) MyIcon.ChevronRight else MyIcon.ChevronDown, colors.textMuted, Tokens.Size.IconSm)
                            Label(group.label, style = Tokens.Type.UiStrong, maxLines = 1)

                            if (group.rows.size > 1) {
                                Label(group.rows.size.toString(), style = Tokens.Type.Meta, color = colors.textMuted)
                            }
                        }

                        Hairline()
                    }
                }

                if (!isCollapsed) {
                    items(group.rows, key = { it.conversation.id }) { entry ->
                        val row = entry.conversation

                        ContentRow(Modifier.fillMaxWidth().hoverHighlight()) {
                            Column(
                                Modifier
                                    .pressable({ onOpen(row.id) }, role = Role.Button, description = "Open ${row.title}", shape = ControlShape)
                                    .padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1),
                                verticalArrangement = Arrangement.Center,
                            ) {
                                Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                                    Label(row.title, Modifier.weight(1f, fill = false), maxLines = 1)

                                    if (row.archivedAt != null) {
                                        Tag("Archived")
                                    }
                                }

                                if (touch) {
                                    Label(
                                        (listOf(entry.project, entry.agent, entry.node, stamp(row.updatedAt, false)) + usageCells(entry.usage).drop(1)).filter { it.isNotEmpty() && it != "Not reported" }.joinToString("  ·  "),
                                        style = Tokens.Type.Meta,
                                        color = colors.textMuted,
                                        maxLines = 1,
                                    )
                                }
                            }

                            if (!touch) {
                                for (column in shown.drop(1)) {
                                    Label(cellText(entry, column, stamp), color = colors.textMuted, maxLines = 1)
                                }
                            }

                            rowMenu(row)
                        }

                        Hairline()
                    }
                }
            }
        }
    }
}
