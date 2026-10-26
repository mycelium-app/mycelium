package mycelium.ui

import androidx.compose.animation.core.snap
import androidx.compose.animation.core.tween
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.ScrollState
import androidx.compose.foundation.gestures.ScrollableState
import androidx.compose.foundation.gestures.scrollBy
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.CompositingStrategy
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.BlendMode
import androidx.compose.foundation.horizontalScroll
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.PointerEventType
import androidx.compose.ui.input.pointer.PointerId
import androidx.compose.ui.input.pointer.PointerType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.SemanticsPropertyKey
import androidx.compose.ui.semantics.SemanticsPropertyReceiver
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

expect val PlatformDrawsScrollbars: Boolean

val ScrollbarShown = SemanticsPropertyKey<Unit>("ScrollbarShown")

private var SemanticsPropertyReceiver.scrollbarShown by ScrollbarShown

private const val LingerMillis = 900L
private const val FadeMillis = 200
private val RestWidth = 4.dp
private val HoverWidth = 8.dp
private val Grab = 14.dp
private val MinThumb = 32.dp
private val Inset = 2.dp

private class Extent(val content: Float, val offset: Float)

private class Thumb(val top: Float, val length: Float, val travel: Float, val scrollable: Float)

private fun thumbFor(extent: Extent, viewport: Float, reverse: Boolean, minLength: Float): Thumb? {
    val scrollable = extent.content - viewport

    if (viewport <= 0f || scrollable <= 0f) return null

    val length = (viewport / extent.content * viewport).coerceIn(minLength.coerceAtMost(viewport), viewport)
    val travel = viewport - length
    val progress = (extent.offset / scrollable).coerceIn(0f, 1f)
    val top = travel * if (reverse) 1f - progress else progress

    return Thumb(top, length, travel, scrollable)
}

@Composable
fun Modifier.verticalScrollWithBar(state: ScrollState = rememberScrollState()): Modifier =
    scrollbar(state, reverse = false) { viewport -> Extent(state.maxValue + viewport, state.value.toFloat()) }.verticalScroll(state)

@Composable
fun Modifier.horizontalScrollFaded(state: ScrollState = rememberScrollState()): Modifier = this
    .graphicsLayer { compositingStrategy = CompositingStrategy.Offscreen }
    .drawWithContent {
        drawContent()
        val edge = minOf(Tokens.Space.S6.toPx(), size.width / 2)

        if (state.canScrollForward) {
            drawRect(Brush.horizontalGradient(listOf(Color.Transparent, Color.Black), startX = size.width - edge, endX = size.width), Offset(size.width - edge, 0f), Size(edge, size.height), blendMode = BlendMode.DstOut)
        }

        if (state.canScrollBackward) {
            drawRect(Brush.horizontalGradient(listOf(Color.Black, Color.Transparent), startX = 0f, endX = edge), Offset.Zero, Size(edge, size.height), blendMode = BlendMode.DstOut)
        }
    }
    .horizontalScroll(state)

@Composable
fun Modifier.scrollbar(state: LazyListState, reverse: Boolean = false): Modifier =
    scrollbar(state, reverse) { _ ->
        val info = state.layoutInfo
        val visible = info.visibleItemsInfo

        if (visible.isEmpty()) {
            Extent(0f, 0f)
        } else {
            val average = visible.sumOf { it.size }.toFloat() / visible.size + info.mainAxisItemSpacing
            val content = average * info.totalItemsCount + info.beforeContentPadding + info.afterContentPadding

            Extent(content, state.firstVisibleItemIndex * average + state.firstVisibleItemScrollOffset)
        }
    }

@Composable
private fun Modifier.scrollbar(state: ScrollableState, reverse: Boolean, extent: (Float) -> Extent): Modifier {
    if (!PlatformDrawsScrollbars) return this

    val colors = MyTheme.colors
    val reducedMotion = LocalReducedMotion.current
    val density = LocalDensity.current
    var hovering by remember { mutableStateOf(false) }
    var dragging by remember { mutableStateOf(false) }
    var lingering by remember { mutableStateOf(false) }
    var viewport by remember { mutableFloatStateOf(0f) }
    val scrolling = state.isScrollInProgress
    val overflows = state.canScrollForward || state.canScrollBackward
    val shown = overflows && (hovering || dragging || lingering)
    val alpha by animateFloatAsState(if (shown) 1f else 0f, if (reducedMotion) snap() else tween(FadeMillis))
    val minLength = with(density) { MinThumb.toPx() }

    LaunchedEffect(scrolling) {
        if (scrolling) {
            lingering = true
        } else {
            delay(LingerMillis)
            lingering = false
        }
    }

    return this
        .onSizeChanged { viewport = it.height.toFloat() }
        .pointerInput(state, reverse) {
            coroutineScope {
                val scope = this
                val grab = Grab.toPx()
                var dragId: PointerId? = null

                awaitPointerEventScope {
                    while (true) {
                        val event = awaitPointerEvent(PointerEventPass.Initial)
                        val change = event.changes.first()

                        if (change.type == PointerType.Mouse) {
                            hovering = event.type != PointerEventType.Exit
                        }

                        when (event.type) {
                            PointerEventType.Press -> {
                                val thumb = thumbFor(extent(size.height.toFloat()), size.height.toFloat(), reverse, minLength)
                                val onThumb = thumb != null && change.position.x >= size.width - grab && change.position.y in thumb.top..thumb.top + thumb.length

                                if (onThumb && change.type == PointerType.Mouse) {
                                    dragId = change.id
                                    dragging = true
                                    change.consume()
                                }
                            }

                            PointerEventType.Move -> {
                                val dragged = event.changes.firstOrNull { it.id == dragId }
                                val thumb = thumbFor(extent(size.height.toFloat()), size.height.toFloat(), reverse, minLength)

                                if (dragged != null && thumb != null && thumb.travel > 0f) {
                                    val direction = if (reverse) -1f else 1f
                                    val delta = (dragged.position.y - dragged.previousPosition.y) / thumb.travel * thumb.scrollable * direction

                                    dragged.consume()
                                    scope.launch { state.scrollBy(delta) }
                                }
                            }

                            PointerEventType.Release -> {
                                if (event.changes.any { it.id == dragId }) {
                                    dragId = null
                                    dragging = false
                                }
                            }
                        }
                    }
                }
            }
        }
        .drawWithContent {
            drawContent()

            val thumb = if (alpha > 0f) thumbFor(extent(size.height), size.height, reverse, minLength) else null

            if (thumb != null) {
                val width = (if (hovering || dragging) HoverWidth else RestWidth).toPx()
                val color = if (hovering || dragging) colors.textMuted else colors.textFaint

                drawRoundRect(
                    color = color,
                    topLeft = Offset(size.width - width - Inset.toPx(), thumb.top),
                    size = Size(width, thumb.length),
                    cornerRadius = CornerRadius(width / 2f),
                    alpha = alpha,
                )
            }
        }
        .semantics { if (shown) scrollbarShown = Unit }
}
