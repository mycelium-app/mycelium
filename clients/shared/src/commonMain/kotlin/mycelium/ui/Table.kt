package mycelium.ui

import androidx.compose.foundation.border
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import kotlin.math.ceil

enum class ColumnAlign { Start, Center, End }

enum class Overflow { Truncate, Wrap }

data class Col(
    val title: String,
    val min: Dp,
    val preferred: Dp? = null,
    val max: Dp? = null,
    val weight: Float = 0f,
    val align: ColumnAlign = ColumnAlign.Start,
    val overflow: Overflow = Overflow.Truncate,
    val numeric: Boolean = false,
    val priority: Int = 0,
)

sealed interface TableCell {
    data class Text(val text: String, val style: TextStyle = Tokens.Type.Ui, val color: Color? = null) : TableCell

    class Slot(val measureAs: String? = null, val extra: Dp = 0.dp, val content: @Composable () -> Unit) : TableCell
}

val ActionColumn = Col("", 32.dp, preferred = 32.dp, align = ColumnAlign.End)

data class ResolvedTable(val widths: List<Int>, val total: Int, val scroll: Boolean)

fun resolveTable(min: List<Int>, natural: List<Int>, weight: List<Float>, available: Int, fill: Boolean = true): ResolvedTable {
    val count = min.size
    val minTotal = min.sum()

    if (minTotal > available) {
        return ResolvedTable(min, minTotal, true)
    }

    val widths = IntArray(count) { maxOf(natural[it], min[it]) }
    val wantedTotal = widths.sum()

    if (wantedTotal <= available) {
        if (!fill || count == 0) {
            return ResolvedTable(widths.toList(), wantedTotal, false)
        }

        val spare = available - wantedTotal
        val weighted = (0 until count).filter { weight[it] > 0f }
        val receivers = weighted.ifEmpty { listOf(0) }
        val weights = receivers.map { if (weighted.isEmpty()) 1f else weight[it] }
        val weightTotal = weights.sum()
        var given = 0

        receivers.forEachIndexed { position, index ->
            val share = if (position == receivers.lastIndex) spare - given else (spare * weights[position] / weightTotal).toInt()
            widths[index] += share
            given += share
        }

        return ResolvedTable(widths.toList(), available, false)
    }

    var excess = wantedTotal - available
    val weightedFirst = (0 until count).filter { weight[it] > 0f }
    val rest = (0 until count).filter { weight[it] <= 0f }

    for (group in listOf(weightedFirst, rest)) {
        val slack = group.sumOf { widths[it] - min[it] }
        val take = minOf(excess, slack)

        if (take <= 0) {
            continue
        }

        var taken = 0

        for (index in group) {
            val share = (take.toLong() * (widths[index] - min[index]) / slack).toInt()
            widths[index] -= share
            taken += share
        }

        for (index in group) {
            if (taken < take && widths[index] > min[index]) {
                widths[index] -= 1
                taken += 1
            }
        }

        excess -= taken
    }

    return ResolvedTable(widths.toList(), available, false)
}

private val LazyMaxHeight = 400.dp

@Composable
fun Table(
    columns: List<Col>,
    rows: Int,
    cell: (row: Int, column: Int) -> TableCell,
    modifier: Modifier = Modifier,
    header: Boolean = true,
    lazy: Boolean = rows > 50,
    fill: Boolean = true,
    card: Boolean = true,
    rowKey: ((Int) -> Any)? = null,
    onRow: ((Int) -> Unit)? = null,
    rowDescription: (Int) -> String = { "Row ${it + 1}" },
    narrowBreakpoint: Dp = 480.dp,
    headerCell: (Int) -> TableCell = { TableCell.Text(columns[it].title, Tokens.Type.MetaStrong) },
    detail: (@Composable (Int) -> Unit)? = null,
) {
    val colors = MyTheme.colors
    val density = LocalDensity.current
    val measurer = rememberTextMeasurer(cacheSize = 512)
    val fonts = LocalFonts.current
    val codeScale = LocalAppearance.current.display.codeSize.scale
    val minHeight = rowHeight()

    BoxWithConstraints(modifier) {
        val hidden = columns.indices.filter { columns[it].priority > 0 && maxWidth < narrowBreakpoint }.toSet()
        val visible = columns.indices.filter { it !in hidden }
        val inset = with(density) { Tokens.Space.S3.roundToPx() }
        val border = with(density) { Tokens.Size.Hairline.roundToPx() }
        val wrapCap = with(density) { Tokens.Size.TableCell.roundToPx() }
        val tnumStyle = { style: TextStyle, numeric: Boolean -> if (numeric) style.copy(fontFeatureSettings = "tnum") else style }

        val natural = remember(columns, rows, cell, headerCell, header, visible, fonts, codeScale, density) {
            val measure = { column: Int, source: TableCell ->
                when (source) {
                    is TableCell.Text -> measurer.measure(source.text, tnumStyle(source.style, columns[column].numeric).themedWith(fonts, codeScale), softWrap = false, maxLines = 1).size.width
                    is TableCell.Slot -> {
                        val text = requireNotNull(source.measureAs) { "A slot in column ${columns[column].title} needs measureAs, or the column needs preferred" }

                        measurer.measure(text, Tokens.Type.Ui.themedWith(fonts, codeScale), softWrap = false, maxLines = 1).size.width + with(density) { source.extra.roundToPx() }
                    }
                }
            }

            visible.map { column ->
                val spec = columns[column]
                val preferred = spec.preferred

                if (preferred != null) {
                    with(density) { preferred.roundToPx() }
                } else {
                    var widest = if (header) measure(column, headerCell(column)) else 0

                    for (row in 0 until rows) {
                        widest = maxOf(widest, measure(column, cell(row, column)))
                    }

                    val capped = if (spec.overflow == Overflow.Wrap) minOf(widest, wrapCap) else widest

                    spec.max?.let { minOf(capped, with(density) { it.roundToPx() }) } ?: capped
                }
            }
        }

        val outerNatural = visible.mapIndexed { position, column -> maxOf(natural[position], with(density) { columns[column].min.roundToPx() }) + inset + if (position == visible.lastIndex) inset else 0 }
        val outerMin = visible.mapIndexed { position, column -> with(density) { columns[column].min.roundToPx() } + inset + if (position == visible.lastIndex) inset else 0 }
        val borderTotal = if (card) border * 2 else 0
        val available = if (constraints.hasBoundedWidth) constraints.maxWidth - borderTotal else outerNatural.sum()
        val resolved = resolveTable(outerMin, outerNatural, visible.map { columns[it].weight }, available, fill)
        val totalWidth = with(density) { resolved.total.toDp() }
        val outerWidth = if (resolved.scroll) maxWidth else with(density) { (resolved.total + borderTotal).toDp() }
        val topAligned = visible.any { columns[it].overflow == Overflow.Wrap }
        val scroll = rememberScrollState()
        val boundedHeight = constraints.hasBoundedHeight
        val hairline = Tokens.Size.Hairline
        val rowLine = colors.border
        val headerBackground = colors.bgPanel

        val renderRow = @Composable { index: Int, head: Boolean ->
            val look = Modifier.drawBehind {
                if (head) {
                    drawRect(headerBackground)
                } else {
                    drawLine(rowLine, Offset(0f, 0f), Offset(size.width, 0f), strokeWidth = hairline.toPx())
                }
            }

            val press = if (!head && onRow != null) Modifier.pressable({ onRow(index) }, description = rowDescription(index)) else Modifier

            Column(Modifier.width(totalWidth).then(look)) {
                TableRow(resolved.widths, minHeight, topAligned, press) {
                    visible.forEachIndexed { position, column ->
                        val spec = columns[column]
                        val source = if (head) headerCell(column) else cell(index, column)

                        CellBox(spec, source, wrapped = spec.overflow == Overflow.Wrap && !head, last = position == visible.lastIndex, muted = head)
                    }
                }

                if (!head) {
                    detail?.invoke(index)
                }
            }
        }

        val shape = if (card) Modifier.border(hairline, colors.border, BlockShape).clip(BlockShape).padding(hairline) else Modifier

        Box(Modifier.width(outerWidth).then(shape)) {
            Column(if (resolved.scroll) Modifier.horizontalScroll(scroll) else Modifier) {
                if (header) {
                    renderRow(-1, true)
                }

                if (lazy) {
                    val bound = if (boundedHeight) Modifier else Modifier.heightIn(max = LazyMaxHeight)

                    LazyColumn(bound.width(totalWidth)) {
                        items(rows, key = rowKey) { index -> renderRow(index, false) }
                    }
                } else {
                    for (index in 0 until rows) {
                        renderRow(index, false)
                    }
                }
            }
        }
    }
}

@Composable
private fun TableRow(widths: List<Int>, minHeight: Dp, topAligned: Boolean, modifier: Modifier, content: @Composable () -> Unit) {
    val verticalPad = if (topAligned) Tokens.Space.S2 else 0.dp

    Layout(content, modifier) { cells, constraints ->
        val placeables = cells.mapIndexed { index, cell -> cell.measure(Constraints(minWidth = widths[index], maxWidth = widths[index])) }
        val pad = verticalPad.roundToPx()
        val height = maxOf(minHeight.roundToPx(), (placeables.maxOfOrNull { it.height } ?: 0) + pad * 2)

        layout(widths.sum(), height) {
            var x = 0

            placeables.forEachIndexed { index, placeable ->
                placeable.place(x, if (topAligned) pad else (height - placeable.height) / 2)
                x += widths[index]
            }
        }
    }
}

@Composable
private fun CellBox(spec: Col, source: TableCell, wrapped: Boolean, last: Boolean, muted: Boolean) {
    val inset = Tokens.Space.S3
    val align = if (spec.numeric) ColumnAlign.End else spec.align

    val alignment = when (align) {
        ColumnAlign.Start -> Alignment.CenterStart
        ColumnAlign.Center -> Alignment.Center
        ColumnAlign.End -> Alignment.CenterEnd
    }

    Box(Modifier.fillMaxWidth().padding(start = inset, end = if (last) inset else 0.dp), contentAlignment = alignment) {
        when (source) {
            is TableCell.Text -> {
                val style = if (spec.numeric) source.style.copy(fontFeatureSettings = "tnum") else source.style

                Label(source.text, style = style, color = source.color ?: if (muted) MyTheme.colors.textMuted else MyTheme.colors.text, maxLines = if (wrapped) Int.MAX_VALUE else 1)
            }

            is TableCell.Slot -> source.content()
        }
    }
}
