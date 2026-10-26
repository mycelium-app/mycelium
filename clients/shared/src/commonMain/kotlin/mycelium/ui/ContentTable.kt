package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.layout.IntrinsicMeasurable
import androidx.compose.ui.layout.IntrinsicMeasureScope
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.layout.Measurable
import androidx.compose.ui.layout.MeasurePolicy
import androidx.compose.ui.layout.MeasureResult
import androidx.compose.ui.layout.MeasureScope
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

data class ColumnSpec(val max: Dp = 240.dp, val min: Dp = 56.dp, val flexible: Boolean = false)

private class ColumnWidths(val columns: List<ColumnSpec>) {
    val natural = IntArray(columns.size)
    val resolved = IntArray(columns.size)
}

private val LocalColumnWidths = compositionLocalOf<ColumnWidths?> { null }

@Composable
fun ContentTable(columns: List<ColumnSpec>, modifier: Modifier = Modifier, card: Boolean = true, content: @Composable () -> Unit) {
    val widths = remember(columns) { ColumnWidths(columns) }
    val gap = Tokens.Space.S3

    CompositionLocalProvider(LocalColumnWidths provides widths) {
        Layout(
            content,
            modifier.fillMaxWidth().then(if (card) Modifier.border(Tokens.Size.Hairline, MyTheme.colors.border, BlockShape).clip(BlockShape) else Modifier),
        ) { children, constraints ->
            widths.natural.fill(0)
            for (child in children) {
                child.maxIntrinsicWidth(Constraints.Infinity)
            }

            val gaps = gap.roundToPx() * (columns.size - 1).coerceAtLeast(0)
            val padding = gap.roundToPx() * 2
            val available = (constraints.maxWidth - gaps - padding).coerceAtLeast(0)
            resolveColumnWidths(columns.map { it.min.roundToPx() }, columns.map { it.max.roundToPx() }, columns.map { it.flexible }, widths.natural, available, widths.resolved)

            val placeables = children.map { it.measure(Constraints(minWidth = constraints.maxWidth, maxWidth = constraints.maxWidth)) }
            val height = placeables.sumOf { it.height }

            layout(constraints.maxWidth, height) {
                var y = 0
                for (placeable in placeables) {
                    placeable.place(0, y)
                    y += placeable.height
                }
            }
        }
    }
}

@Composable
fun ContentRow(modifier: Modifier = Modifier, header: Boolean = false, minHeight: Dp = rowHeight(), content: @Composable () -> Unit) {
    val widths = LocalColumnWidths.current
    val gap = Tokens.Space.S3
    val look = if (header) Modifier.background(MyTheme.colors.bgPanel) else Modifier
    val policy = remember(widths, gap) { ContentRowPolicy(widths, gap) }

    Layout(content, Modifier.heightIn(min = minHeight).then(modifier).then(look).padding(horizontal = gap), policy)
}

private class ContentRowPolicy(private val widths: ColumnWidths?, private val gap: Dp) : MeasurePolicy {
    override fun MeasureScope.measure(measurables: List<Measurable>, constraints: Constraints): MeasureResult {
        val gapPx = gap.roundToPx()
        val resolved = widths?.resolved?.takeIf { it.size == measurables.size && it.sum() > 0 }
            ?: IntArray(measurables.size) { (constraints.maxWidth - gapPx * (measurables.size - 1)).coerceAtLeast(0) / measurables.size.coerceAtLeast(1) }

        val placeables = measurables.mapIndexed { index, cell -> cell.measure(Constraints(minWidth = resolved[index], maxWidth = resolved[index])) }
        val height = maxOf(placeables.maxOfOrNull { it.height } ?: 0, constraints.minHeight)

        return layout(constraints.maxWidth, height) {
            var x = 0
            placeables.forEachIndexed { index, placeable ->
                placeable.place(x, (height - placeable.height) / 2)
                x += resolved[index] + gapPx
            }
        }
    }

    override fun IntrinsicMeasureScope.maxIntrinsicWidth(measurables: List<IntrinsicMeasurable>, height: Int): Int {
        var total = 0
        measurables.forEachIndexed { index, cell ->
            val width = cell.maxIntrinsicWidth(Constraints.Infinity)
            total += width

            if (widths != null && index < widths.natural.size) {
                widths.natural[index] = maxOf(widths.natural[index], width)
            }
        }

        return total + gap.roundToPx() * (measurables.size - 1).coerceAtLeast(0)
    }

    override fun IntrinsicMeasureScope.minIntrinsicWidth(measurables: List<IntrinsicMeasurable>, height: Int): Int = maxIntrinsicWidth(measurables, height)

    override fun IntrinsicMeasureScope.minIntrinsicHeight(measurables: List<IntrinsicMeasurable>, width: Int): Int = measurables.maxOfOrNull { it.minIntrinsicHeight(width) } ?: 0

    override fun IntrinsicMeasureScope.maxIntrinsicHeight(measurables: List<IntrinsicMeasurable>, width: Int): Int = measurables.maxOfOrNull { it.maxIntrinsicHeight(width) } ?: 0
}

internal fun resolveColumnWidths(
    min: List<Int>,
    max: List<Int>,
    flexible: List<Boolean>,
    natural: IntArray,
    available: Int,
    into: IntArray,
) {
    val count = min.size
    val wanted = IntArray(count) { index -> if (flexible[index]) natural[index].coerceAtLeast(min[index]) else natural[index].coerceIn(min[index], max[index]) }
    val flexibleIndex = flexible.indexOf(true)
    val total = wanted.sum()

    if (total <= available) {
        wanted.copyInto(into)
        if (flexibleIndex >= 0) {
            into[flexibleIndex] += available - total
        }

        return
    }

    var excess = total - available

    if (flexibleIndex >= 0) {
        val give = minOf(excess, wanted[flexibleIndex] - min[flexibleIndex])
        wanted[flexibleIndex] -= give
        excess -= give
    }

    val slack = IntArray(count) { index -> if (index == flexibleIndex) 0 else wanted[index] - min[index] }
    val slackTotal = slack.sum()

    if (excess > 0 && slackTotal > 0) {
        val give = minOf(excess, slackTotal)
        for (index in 0 until count) {
            wanted[index] -= (give.toLong() * slack[index] / slackTotal).toInt()
        }
        excess -= give
    }

    if (excess > 0) {
        val sum = wanted.sum().coerceAtLeast(1)
        for (index in 0 until count) {
            wanted[index] = (wanted[index].toLong() * available / sum).toInt()
        }
    }

    wanted.copyInto(into)
}
