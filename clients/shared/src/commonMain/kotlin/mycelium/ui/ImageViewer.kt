package mycelium.ui

import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.focusable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.calculateCentroid
import androidx.compose.foundation.gestures.calculatePan
import androidx.compose.foundation.gestures.calculateZoom
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
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
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.input.pointer.PointerEventType
import androidx.compose.ui.input.pointer.PointerType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.pointer.positionChanged
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import kotlin.math.min
import kotlin.math.pow
import kotlin.math.roundToInt
import mycelium.state.ThreadImage

const val MinImageZoom = 1f
const val MaxImageZoom = 8f
const val DoubleTapImageZoom = 2.5f
const val KeyImageZoomStep = 1.25f
const val WheelImageZoomStep = 1.15f
const val SwipeToCloseDistance = 120f

data class ViewTransform(val scale: Float = MinImageZoom, val offset: Offset = Offset.Zero)

fun fitSize(image: Size, window: Size): Size {
    if (image.width <= 0f || image.height <= 0f || window.width <= 0f || window.height <= 0f) {
        return Size.Zero
    }

    val scale = min(window.width / image.width, window.height / image.height)

    return Size(image.width * scale, image.height * scale)
}

fun clampScale(scale: Float): Float = scale.coerceIn(MinImageZoom, MaxImageZoom)

fun clampOffset(offset: Offset, scale: Float, fitted: Size, window: Size): Offset {
    val limitX = maxOf(0f, (fitted.width * scale - window.width) / 2)
    val limitY = maxOf(0f, (fitted.height * scale - window.height) / 2)

    return Offset(clampAxis(offset.x, limitX), clampAxis(offset.y, limitY))
}

private fun clampAxis(value: Float, limit: Float): Float = if (limit == 0f) 0f else value.coerceIn(-limit, limit)

fun zoomAbout(transform: ViewTransform, newScale: Float, anchor: Offset, fitted: Size, window: Size): ViewTransform {
    val scale = clampScale(newScale)
    val ratio = scale / transform.scale
    val offset = anchor - (anchor - transform.offset) * ratio

    return ViewTransform(scale, clampOffset(offset, scale, fitted, window))
}

fun panBy(transform: ViewTransform, delta: Offset, fitted: Size, window: Size): ViewTransform =
    transform.copy(offset = clampOffset(transform.offset + delta, transform.scale, fitted, window))

fun toggleZoom(transform: ViewTransform, anchor: Offset, fitted: Size, window: Size): ViewTransform =
    if (transform.scale > MinImageZoom) ViewTransform() else zoomAbout(transform, DoubleTapImageZoom, anchor, fitted, window)

fun imageBounds(transform: ViewTransform, fitted: Size): Rect {
    val size = Size(fitted.width * transform.scale, fitted.height * transform.scale)

    return Rect(Offset(transform.offset.x - size.width / 2, transform.offset.y - size.height / 2), size)
}

fun stepIndex(index: Int, delta: Int, count: Int): Int = (index + delta).coerceIn(0, maxOf(0, count - 1))

fun zoomPercent(scale: Float): String = "${(scale * 100).roundToInt()}%"

@Composable
fun ImageViewer(images: List<ThreadImage>, start: Int, onClose: () -> Unit) {
    var index by remember { mutableStateOf(start.coerceIn(0, maxOf(0, images.lastIndex))) }
    var transform by remember { mutableStateOf(ViewTransform()) }
    var window by remember { mutableStateOf(Size.Zero) }
    val image = images[index]
    val loaded by rememberLoadedImage(image)
    val bitmap = loaded.second
    val focus = remember { FocusRequester() }
    val center = Offset(window.width / 2, window.height / 2)

    fun fitted(): Size = bitmap?.let { fitSize(Size(it.width.toFloat(), it.height.toFloat()), window) } ?: Size.Zero

    fun zoomTo(scale: Float, anchor: Offset = Offset.Zero) {
        transform = zoomAbout(transform, scale, anchor, fitted(), window)
    }

    fun step(delta: Int) {
        val next = stepIndex(index, delta, images.size)

        if (next != index) {
            index = next
            transform = ViewTransform()
        }
    }

    LaunchedEffect(Unit) { focus.requestFocus() }

    Box(
        Modifier
            .fillMaxSize()
            .background(Color.Black.copy(alpha = 0.9f))
            .onSizeChanged { window = Size(it.width.toFloat(), it.height.toFloat()) }
            .focusRequester(focus)
            .focusable()
            .onPreviewKeyEvent { event ->
                if (event.type != KeyEventType.KeyDown) {
                    return@onPreviewKeyEvent false
                }

                when (event.key) {
                    Key.Escape -> onClose()
                    Key.DirectionLeft -> step(-1)
                    Key.DirectionRight -> step(1)
                    Key.Plus, Key.Equals, Key.NumPadAdd -> zoomTo(transform.scale * KeyImageZoomStep)
                    Key.Minus, Key.NumPadSubtract -> zoomTo(transform.scale / KeyImageZoomStep)
                    Key.Zero, Key.NumPad0 -> transform = ViewTransform()
                    else -> return@onPreviewKeyEvent false
                }

                true
            }
            .pointerInput(Unit) {
                awaitPointerEventScope {
                    while (true) {
                        val event = awaitPointerEvent()

                        if (event.type == PointerEventType.Scroll) {
                            val change = event.changes.first()

                            zoomTo(transform.scale * WheelImageZoomStep.pow(-change.scrollDelta.y), change.position - Offset(window.width / 2, window.height / 2))
                            event.changes.forEach { it.consume() }
                        }
                    }
                }
            }
            .pointerInput(Unit) {
                detectTapGestures(
                    onDoubleTap = { point ->
                        transform = toggleZoom(transform, point - Offset(window.width / 2, window.height / 2), fitted(), window)
                    },
                    onTap = { point ->
                        val inWindow = point - Offset(window.width / 2, window.height / 2)

                        if (!imageBounds(transform, fitted()).contains(inWindow)) {
                            onClose()
                        }
                    },
                )
            }
            .pointerInput(Unit) {
                awaitEachGesture {
                    val first = awaitFirstDown(requireUnconsumed = false)
                    val touch = first.type == PointerType.Touch
                    var swiped = 0f

                    do {
                        val event = awaitPointerEvent()
                        val zoom = event.calculateZoom()
                        val pan = event.calculatePan()

                        if (zoom != 1f || pan != Offset.Zero) {
                            if (touch && transform.scale <= MinImageZoom && zoom == 1f) {
                                swiped += pan.y

                                if (swiped > SwipeToCloseDistance) {
                                    onClose()

                                    return@awaitEachGesture
                                }
                            } else {
                                val anchor = event.calculateCentroid(useCurrent = false) - Offset(window.width / 2, window.height / 2)
                                val zoomed = zoomAbout(transform, transform.scale * zoom, anchor, fitted(), window)

                                transform = panBy(zoomed, pan, fitted(), window)
                            }

                            event.changes.forEach { if (it.positionChanged()) it.consume() }
                        }
                    } while (event.changes.any { it.pressed })
                }
            }
            .semantics { contentDescription = "Image: ${image.name}" },
        contentAlignment = Alignment.Center,
    ) {
        if (bitmap != null) {
            Image(
                bitmap,
                contentDescription = image.name,
                Modifier.fillMaxSize().graphicsLayer {
                    scaleX = transform.scale
                    scaleY = transform.scale
                    translationX = transform.offset.x
                    translationY = transform.offset.y
                },
                contentScale = ContentScale.Fit,
            )
        } else {
            Label(if (loaded.first) "${image.name} could not be shown" else "Loading ${image.name}", style = Tokens.Type.Meta, color = Color.White)
        }

        Row(
            Modifier.align(Alignment.TopCenter).fillMaxWidth().padding(Tokens.Space.S3),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (images.size > 1) {
                Label("${index + 1} of ${images.size}", color = Color.White)
            }

            Box(Modifier.weight(1f))
            Label(zoomPercent(transform.scale), style = Tokens.Type.Meta, color = Color.White)
            MyButton({ zoomTo(transform.scale / KeyImageZoomStep) }, icon = MyIcon.Minus, description = "Zoom out", variant = ButtonVariant.Outline)
            MyButton({ zoomTo(transform.scale * KeyImageZoomStep) }, icon = MyIcon.Plus, description = "Zoom in", variant = ButtonVariant.Outline)
            MyButton(onClose, icon = MyIcon.Close, description = "Close image", variant = ButtonVariant.Outline)
        }

        if (index > 0) {
            Box(Modifier.align(Alignment.CenterStart).padding(Tokens.Space.S3)) {
                MyButton({ step(-1) }, icon = MyIcon.ChevronLeft, description = "Previous image", variant = ButtonVariant.Outline)
            }
        }

        if (index < images.lastIndex) {
            Box(Modifier.align(Alignment.CenterEnd).padding(Tokens.Space.S3)) {
                MyButton({ step(1) }, icon = MyIcon.ChevronRight, description = "Next image", variant = ButtonVariant.Outline)
            }
        }
    }
}
