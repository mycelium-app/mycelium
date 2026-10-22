package mycelium.ui

import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.semantics.ProgressBarRangeInfo
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.progressBarRangeInfo
import androidx.compose.ui.semantics.semantics

private const val SweepMillis = 1200
private const val SegmentFraction = 0.35f

@Composable
fun IndeterminateProgress(description: String, modifier: Modifier = Modifier) {
    val colors = MyTheme.colors
    val start = if (LocalReducedMotion.current) {
        (1f - SegmentFraction) / 2f
    } else {
        val sweep by rememberInfiniteTransition().animateFloat(-SegmentFraction, 1f, infiniteRepeatable(tween(SweepMillis, easing = LinearEasing), RepeatMode.Restart))

        sweep
    }

    Canvas(
        modifier
            .fillMaxWidth()
            .height(Tokens.Size.ProgressBar)
            .clipToBounds()
            .semantics {
                contentDescription = description
                progressBarRangeInfo = ProgressBarRangeInfo.Indeterminate
            },
    ) {
        val radius = CornerRadius(size.height / 2f)

        drawRoundRect(colors.border, cornerRadius = radius)
        drawRoundRect(colors.accent, Offset(start * size.width, 0f), Size(SegmentFraction * size.width, size.height), radius)
    }
}
