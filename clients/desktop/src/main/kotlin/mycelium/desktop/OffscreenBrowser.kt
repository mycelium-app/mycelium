package mycelium.desktop

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asComposeImageBitmap
import androidx.compose.ui.input.pointer.PointerIcon
import org.cef.browser.CefBrowser
import org.cef.callback.CefDragData
import org.cef.handler.CefRenderHandler
import org.cef.handler.CefScreenInfo
import org.jetbrains.skia.Bitmap
import org.jetbrains.skia.ColorAlphaType
import org.jetbrains.skia.ColorType
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
import kotlin.math.roundToInt

private const val WheelNotchPixels = 40

fun viewSize(widthPx: Int, heightPx: Int, scale: Double): Pair<Int, Int> =
    (widthPx / scale).roundToInt().coerceAtLeast(1) to (heightPx / scale).roundToInt().coerceAtLeast(1)

fun browserMouseEvent(source: Component, native: MouseEvent, position: Offset, scale: Double): MouseEvent {
    val x = (position.x / scale).roundToInt()
    val y = (position.y / scale).roundToInt()

    if (native is MouseWheelEvent) {
        val rotation = -native.preciseWheelRotation * WheelNotchPixels

        return MouseWheelEvent(source, native.id, native.`when`, native.modifiersEx, x, y, x, y, native.clickCount, native.isPopupTrigger, native.scrollType, native.scrollAmount, rotation.roundToInt(), rotation)
    }

    return MouseEvent(source, native.id, native.`when`, native.modifiersEx, x, y, x, y, native.clickCount, native.isPopupTrigger, native.button)
}

class OffscreenBrowser : CefRenderHandler {
    val source: Component = Canvas()

    var browser: CefBrowser? = null
        set(value) {
            field = value
            value?.wasResized(viewWidth, viewHeight)
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
    private val bitmap = Bitmap()
    private var image: ImageBitmap? = null

    fun resize(widthPx: Int, heightPx: Int, deviceScale: Double) {
        val (width, height) = viewSize(widthPx, heightPx, deviceScale)
        val rescaled = deviceScale != scale

        scale = deviceScale
        viewWidth = width
        viewHeight = height

        if (rescaled) {
            browser?.notifyScreenInfoChanged()
        }

        browser?.wasResized(width, height)
    }

    fun place(origin: Point) {
        screenOrigin = origin
    }

    fun mouse(native: MouseEvent, position: Offset) {
        val forwarded = browserMouseEvent(source, native, position, scale)

        if (forwarded is MouseWheelEvent) {
            browser?.sendMouseWheelEvent(forwarded)
        } else {
            browser?.sendMouseEvent(forwarded)
        }
    }

    fun key(native: KeyEvent) {
        browser?.sendKeyEvent(native)
    }

    fun frame(): ImageBitmap? = synchronized(lock) {
        if (frameWidth <= 0 || frameHeight <= 0) {
            return null
        }

        if (bitmap.width != frameWidth || bitmap.height != frameHeight) {
            bitmap.allocPixels(ImageInfo(frameWidth, frameHeight, ColorType.BGRA_8888, ColorAlphaType.PREMUL))
            image = bitmap.asComposeImageBitmap()
        }

        bitmap.installPixels(pixels)
        bitmap.notifyPixelsChanged()

        image
    }

    override fun getViewRect(browser: CefBrowser): Rectangle = Rectangle(0, 0, viewWidth, viewHeight)

    override fun getScreenInfo(browser: CefBrowser, screenInfo: CefScreenInfo): Boolean {
        screenInfo.device_scale_factor = scale
        screenInfo.depth = 32
        screenInfo.depth_per_component = 8

        return true
    }

    override fun getScreenPoint(browser: CefBrowser, viewPoint: Point): Point = Point(screenOrigin.x + viewPoint.x, screenOrigin.y + viewPoint.y)

    override fun getDeviceScaleFactor(browser: CefBrowser): Double = scale

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
        }

        SwingUtilities.invokeLater { frames++ }
    }

    override fun onCursorChange(browser: CefBrowser, cursorType: Int): Boolean {
        val shown = PointerIcon(Cursor.getPredefinedCursor(cursorType.coerceIn(Cursor.DEFAULT_CURSOR, Cursor.MOVE_CURSOR)))

        SwingUtilities.invokeLater { cursor = shown }

        return true
    }

    override fun startDragging(browser: CefBrowser, dragData: CefDragData, mask: Int, x: Int, y: Int): Boolean = false

    override fun updateDragCursor(browser: CefBrowser, operation: Int) = Unit
}
