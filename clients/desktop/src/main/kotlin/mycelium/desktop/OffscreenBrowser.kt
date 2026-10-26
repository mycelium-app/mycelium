package mycelium.desktop

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.toComposeImageBitmap
import androidx.compose.ui.input.pointer.PointerIcon
import org.cef.browser.CefBrowser
import org.cef.browser.CefFrame
import org.cef.callback.CefDragData
import org.cef.handler.CefLoadHandlerAdapter
import org.cef.handler.CefRenderHandler
import org.cef.handler.CefScreenInfo
import org.jetbrains.skia.ColorAlphaType
import org.jetbrains.skia.ColorType
import org.jetbrains.skia.Image
import org.jetbrains.skia.ImageInfo
import java.awt.Canvas
import java.awt.Component
import java.awt.Cursor
import java.awt.Point
import java.awt.Rectangle
import java.awt.event.KeyEvent
import java.awt.event.MouseEvent
import java.awt.event.MouseWheelEvent
import java.nio.ByteBuffer
import javax.swing.SwingUtilities
import kotlin.math.ln
import kotlin.math.roundToInt

private const val WheelNotchPixels = 40
private const val ZoomStep = 1.2

fun viewSize(widthPx: Int, heightPx: Int): Pair<Int, Int> =
    widthPx.coerceAtLeast(1) to heightPx.coerceAtLeast(1)

fun pageZoomLevel(scale: Double): Double = ln(scale) / ln(ZoomStep)

fun awtCursorType(cursorType: Int): Int =
    if (cursorType in Cursor.DEFAULT_CURSOR..Cursor.MOVE_CURSOR) cursorType else Cursor.DEFAULT_CURSOR

fun browserMouseEvent(source: Component, native: MouseEvent, position: Offset, scale: Double): MouseEvent {
    val x = position.x.roundToInt()
    val y = position.y.roundToInt()

    if (native is MouseWheelEvent) {
        val rotation = -native.preciseWheelRotation * WheelNotchPixels * scale

        return MouseWheelEvent(source, native.id, native.`when`, native.modifiersEx, x, y, x, y, native.clickCount, native.isPopupTrigger, native.scrollType, native.scrollAmount, rotation.roundToInt(), rotation)
    }

    return MouseEvent(source, native.id, native.`when`, native.modifiersEx, x, y, x, y, native.clickCount, native.isPopupTrigger, native.button)
}

class OffscreenBrowser : CefRenderHandler {
    val source: Component = Canvas()
    val contextMenu = BrowserContextMenu()

    var browser: CefBrowser? = null
        set(value) {
            field = value
            value?.wasResized(viewWidth, viewHeight)
        }

    val loaded = object : CefLoadHandlerAdapter() {
        override fun onLoadEnd(browser: CefBrowser, frame: CefFrame, httpStatusCode: Int) {
            if (frame.isMain) {
                applyZoom()
            }
        }
    }

    var cursor by mutableStateOf(PointerIcon.Default)
        private set

    var frames by mutableIntStateOf(0)
        private set

    private var scale = 1.0
    private var viewWidth = 1
    private var viewHeight = 1
    private var screenOrigin = Point(0, 0)
    private val lock = Any()
    private var pixels = ByteArray(0)
    private var frameWidth = 0
    private var frameHeight = 0
    private var image: ImageBitmap? = null
    private var imageStale = false

    fun resize(widthPx: Int, heightPx: Int, uiScale: Double) {
        val (width, height) = viewSize(widthPx, heightPx)
        val rescaled = uiScale != scale

        scale = uiScale
        viewWidth = width
        viewHeight = height

        if (rescaled) {
            applyZoom()
        }

        browser?.wasResized(width, height)
    }

    fun applyZoom() {
        browser?.zoomLevel = pageZoomLevel(scale)
    }

    fun place(origin: Point, owner: Component? = null, screenScale: Double = 1.0) {
        screenOrigin = origin
        contextMenu.origin = origin
        contextMenu.owner = owner
        contextMenu.screenScale = screenScale
    }

    fun mouse(native: MouseEvent, position: Offset) {
        val window = SwingUtilities.getWindowAncestor(native.component)

        if (window != null) {
            contextMenu.owner = window
            contextMenu.origin = Point(native.xOnScreen - (position.x / contextMenu.screenScale).toInt(), native.yOnScreen - (position.y / contextMenu.screenScale).toInt())
        }

        val forwarded = browserMouseEvent(source, native, position, scale)

        if (forwarded is MouseWheelEvent) {
            browser?.sendMouseWheelEvent(forwarded)
        } else {
            browser?.sendMouseEvent(forwarded)
        }
    }

    fun key(native: KeyEvent) {
        contextMenu.owner = SwingUtilities.getWindowAncestor(native.component) ?: contextMenu.owner
        browser?.sendKeyEvent(native)
    }

    fun frame(): ImageBitmap? = synchronized(lock) {
        if (frameWidth <= 0 || frameHeight <= 0) {
            return null
        }

        if (imageStale || image == null) {
            val info = ImageInfo(frameWidth, frameHeight, ColorType.BGRA_8888, ColorAlphaType.PREMUL)

            image = Image.makeRaster(info, pixels, frameWidth * 4).toComposeImageBitmap()
            imageStale = false
        }

        image
    }

    override fun getViewRect(browser: CefBrowser): Rectangle = Rectangle(0, 0, viewWidth, viewHeight)

    override fun getScreenInfo(browser: CefBrowser, screenInfo: CefScreenInfo): Boolean {
        screenInfo.device_scale_factor = 1.0
        screenInfo.depth = 32
        screenInfo.depth_per_component = 8

        return true
    }

    override fun getScreenPoint(browser: CefBrowser, viewPoint: Point): Point = Point(screenOrigin.x + viewPoint.x, screenOrigin.y + viewPoint.y)

    override fun getDeviceScaleFactor(browser: CefBrowser): Double = 1.0

    override fun onPopupShow(browser: CefBrowser, show: Boolean) = Unit

    override fun onPopupSize(browser: CefBrowser, size: Rectangle) = Unit

    override fun onPaint(browser: CefBrowser, popup: Boolean, dirtyRects: Array<out Rectangle>, buffer: ByteBuffer, width: Int, height: Int) {
        if (popup || width <= 0 || height <= 0) {
            return
        }

        synchronized(lock) {
            val length = width * height * 4

            if (pixels.size != length) {
                pixels = ByteArray(length)
            }

            buffer.duplicate().apply { rewind() }.get(pixels, 0, minOf(length, buffer.capacity()))
            frameWidth = width
            frameHeight = height
            imageStale = true
        }

        SwingUtilities.invokeLater { frames++ }
    }

    override fun onCursorChange(browser: CefBrowser, cursorType: Int): Boolean {
        val shown = PointerIcon(Cursor.getPredefinedCursor(awtCursorType(cursorType)))

        SwingUtilities.invokeLater { cursor = shown }

        return true
    }

    override fun startDragging(browser: CefBrowser, dragData: CefDragData, mask: Int, x: Int, y: Int): Boolean = false

    override fun updateDragCursor(browser: CefBrowser, operation: Int) = Unit
}
