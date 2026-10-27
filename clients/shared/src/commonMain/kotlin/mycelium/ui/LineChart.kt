package mycelium.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.unit.dp

private val LineChartHeight = 96.dp
private const val LineWidth = 2f
private const val DashLength = 6f

@Composable
fun PercentLineChart(points: List<Pair<Long, Double>>, from: Long, until: Long, description: String, fromLabel: String, untilLabel: String, modifier: Modifier = Modifier) {
    val colors = MyTheme.colors

    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            Column(Modifier.height(LineChartHeight), verticalArrangement = Arrangement.SpaceBetween) {
                Label("100%", style = Tokens.Type.Meta, color = colors.textMuted)
                Label("0%", style = Tokens.Type.Meta, color = colors.textMuted)
            }

            PercentLine(points, from, until, description, Modifier.weight(1f))
        }

        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            Label(fromLabel, style = Tokens.Type.Meta, color = colors.textMuted)
            Label(untilLabel, style = Tokens.Type.Meta, color = colors.textMuted)
        }
    }
}

@Composable
private fun PercentLine(points: List<Pair<Long, Double>>, from: Long, until: Long, description: String, modifier: Modifier) {
    val colors = MyTheme.colors

    Canvas(modifier.height(LineChartHeight).clearAndSetSemantics { contentDescription = description }) {
        val span = (until - from).coerceAtLeast(1L).toFloat()
        val top = LineWidth
        val bottom = size.height - LineWidth

        fun at(time: Long, percent: Double) = Offset(((time - from) / span * size.width).coerceIn(0f, size.width), bottom - (bottom - top) * (percent.coerceIn(0.0, 100.0) / 100.0).toFloat())

        drawLine(colors.border, Offset(0f, size.height), Offset(size.width, size.height), strokeWidth = 1f)
        drawLine(colors.textMuted, Offset(0f, top), Offset(size.width, top), strokeWidth = 1f, pathEffect = PathEffect.dashPathEffect(floatArrayOf(DashLength, DashLength)))

        points.zipWithNext().forEach { (start, end) ->
            drawLine(colors.statusWorking, at(start.first, start.second), at(end.first, end.second), strokeWidth = LineWidth, cap = StrokeCap.Round)
        }

        points.singleOrNull()?.let { only -> drawCircle(colors.statusWorking, LineWidth * 1.5f, at(only.first, only.second)) }
    }
}
