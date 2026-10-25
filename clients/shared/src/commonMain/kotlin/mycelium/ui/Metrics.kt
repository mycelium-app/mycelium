package mycelium.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRowScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.semantics.ProgressBarRangeInfo
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.progressBarRangeInfo
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp

data class TableColumn(val title: String, val weight: Float = 1f)

private val ChartHeight = 96.dp
private val MeterHeight = 6.dp
private const val AttentionPercent = 90

private val KpiWidth = 220.dp

@OptIn(ExperimentalLayoutApi::class)
fun FlowRowScope.kpiSlot(): Modifier = Modifier.weight(1f).widthIn(min = KpiWidth).fillMaxRowHeight()

@Composable
fun Kpi(title: String, value: String, modifier: Modifier = Modifier, caption: String? = null, meterPercent: Int? = null) {
    val colors = MyTheme.colors

    Column(
        modifier.border(Tokens.Size.Hairline, colors.border, BlockShape).background(colors.bgPanel, BlockShape).padding(Tokens.Space.S3),
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1),
    ) {
        Label(title, style = Tokens.Type.Meta, color = colors.textMuted)
        Label(value, style = Tokens.Type.Display)

        if (meterPercent != null) {
            Meter(meterPercent, "$title ${meterPercent}%")
        }

        if (caption != null) {
            Label(caption, style = Tokens.Type.Meta, color = colors.textMuted)
        }
    }
}

private val MarkerWidth = 2.dp

@Composable
fun Meter(percent: Int, description: String, modifier: Modifier = Modifier, attention: Boolean = false, markerPercent: Int? = null) {
    val colors = MyTheme.colors
    val fill = when {
        percent >= AttentionPercent -> colors.statusFailed
        attention -> colors.statusAttention
        else -> colors.accent
    }

    Canvas(
        modifier
            .fillMaxWidth()
            .height(MeterHeight)
            .semantics {
                contentDescription = description
                progressBarRangeInfo = ProgressBarRangeInfo(percent.toFloat(), 0f..100f)
            },
    ) {
        val radius = CornerRadius(size.height / 2f)

        drawRoundRect(colors.border, cornerRadius = radius)
        drawRoundRect(fill, Offset.Zero, Size(size.width * percent.coerceIn(0, 100) / 100f, size.height), radius)

        if (markerPercent != null) {
            val x = size.width * markerPercent.coerceIn(0, 100) / 100f

            drawLine(colors.text, Offset(x, 0f), Offset(x, size.height), strokeWidth = MarkerWidth.toPx())
        }
    }
}

@Composable
fun BarChart(values: List<Long>, description: String, modifier: Modifier = Modifier, ceiling: Long? = null) {
    val colors = MyTheme.colors

    Canvas(modifier.fillMaxWidth().height(ChartHeight).clearAndSetSemantics { contentDescription = description }) {
        drawLine(colors.border, Offset(0f, size.height), Offset(size.width, size.height), strokeWidth = 1f)

        val peak = ceiling ?: values.maxOrNull()?.coerceAtLeast(1L) ?: return@Canvas
        val slot = size.width / values.size
        val gap = if (slot > 6f) 2f else 0f

        values.forEachIndexed { index, value ->
            val height = size.height * value / peak

            drawRoundRect(colors.statusWorking, Offset(index * slot + gap / 2f, size.height - height), Size((slot - gap).coerceAtLeast(1f), height), CornerRadius(1f))
        }
    }
}

@Composable
fun DataTable(
    title: String,
    columns: List<TableColumn>,
    rows: List<List<String>>,
    empty: String,
    modifier: Modifier = Modifier,
    onRow: ((Int) -> Unit)? = null,
    rowDescription: (Int) -> String = { rows[it].joinToString(", ") },
    trailing: (@Composable (Int) -> Unit)? = null,
    monospaceColumns: Set<Int> = emptySet(),
) {
    val colors = MyTheme.colors

    Column(modifier.border(Tokens.Size.Hairline, colors.border, BlockShape).background(colors.bgPanel, BlockShape)) {
        Label(title, Modifier.padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2).semantics { heading() }, Tokens.Type.UiStrong)
        Hairline()

        if (rows.isEmpty()) {
            Label(empty, Modifier.padding(Tokens.Space.S3), color = colors.textMuted)

            return@Column
        }

        val specs = columns.mapIndexed { position, _ -> ColumnSpec(flexible = position == 0) } + listOfNotNull(trailing?.let { ColumnSpec(min = TrailingWidth, max = TrailingWidth) })

        ContentTable(specs, card = false) {
            ContentRow {
                for (column in columns) {
                    Label(column.title, style = Tokens.Type.MetaStrong, color = colors.textMuted, maxLines = 1)
                }

                if (trailing != null) {
                    Box(Modifier)
                }
            }

            rows.forEachIndexed { index, cells ->
                Hairline()

                val clickable = if (onRow != null) Modifier.pressable({ onRow(index) }, description = rowDescription(index)) else Modifier

                ContentRow(clickable) {
                    columns.forEachIndexed { position, _ ->
                        Label(
                            cells.getOrElse(position) { "" },
                            style = if (position in monospaceColumns) Tokens.Type.Code else Tokens.Type.Ui,
                            color = if (position == 0) colors.text else colors.textMuted,
                            maxLines = 1,
                        )
                    }

                    if (trailing != null) {
                        Box(contentAlignment = Alignment.CenterEnd) { trailing(index) }
                    }
                }
            }
        }
    }
}

private val TrailingWidth = 72.dp
