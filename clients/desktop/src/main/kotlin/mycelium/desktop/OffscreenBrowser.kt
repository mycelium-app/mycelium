package mycelium.desktop

import org.cef.browser.CefBrowser
import org.cef.callback.CefDragData
import org.cef.handler.CefRenderHandler
import org.cef.handler.CefScreenInfo
import java.awt.Cursor
import java.awt.Graphics
import java.awt.Point
import java.awt.Rectangle
import java.awt.event.ComponentAdapter
import java.awt.event.ComponentEvent
import java.awt.event.KeyEvent
import java.awt.event.KeyListener
import java.awt.event.MouseEvent
import java.awt.event.MouseListener
import java.awt.event.MouseMotionListener
import java.awt.event.MouseWheelEvent
import java.awt.image.BufferedImage
import java.awt.image.DataBufferInt
import java.nio.ByteBuffer
import java.nio.ByteOrder
import javax.swing.JPanel

class OffscreenBrowserPanel : JPanel(), CefRenderHandler {
    var browser: CefBrowser? = null
        set(value) {
            field = value
            value?.wasResized(width, height)
        }

    private var frame: BufferedImage? = null

    init {
        isFocusable = true
        isOpaque = true

        addComponentListener(object : ComponentAdapter() {
            override fun componentResized(event: ComponentEvent) {
                browser?.wasResized(width, height)
            }
        })

        val mouse = object : MouseListener, MouseMotionListener {
            override fun mouseClicked(event: MouseEvent) = forward(event)
            override fun mouseEntered(event: MouseEvent) = forward(event)
            override fun mouseExited(event: MouseEvent) = forward(event)
            override fun mouseDragged(event: MouseEvent) = forward(event)
            override fun mouseMoved(event: MouseEvent) = forward(event)
            override fun mouseReleased(event: MouseEvent) = forward(event)

            override fun mousePressed(event: MouseEvent) {
                requestFocusInWindow()
                browser?.setFocus(true)
                forward(event)
            }
        }

        addMouseListener(mouse)
        addMouseMotionListener(mouse)
        addMouseWheelListener { event: MouseWheelEvent -> browser?.sendMouseWheelEvent(event) }
        addKeyListener(object : KeyListener {
            override fun keyTyped(event: KeyEvent) = forward(event)
            override fun keyPressed(event: KeyEvent) = forward(event)
            override fun keyReleased(event: KeyEvent) = forward(event)
        })
    }

    private fun forward(event: MouseEvent) {
        browser?.sendMouseEvent(event)
    }

    private fun forward(event: KeyEvent) {
        browser?.sendKeyEvent(event)
    }

    override fun paintComponent(graphics: Graphics) {
        super.paintComponent(graphics)
        val shown = synchronized(this) { frame } ?: return

        graphics.drawImage(shown, 0, 0, width, height, null)
    }

    override fun getViewRect(browser: CefBrowser): Rectangle = Rectangle(0, 0, width.coerceAtLeast(1), height.coerceAtLeast(1))

    override fun getScreenInfo(browser: CefBrowser, screenInfo: CefScreenInfo): Boolean {
        screenInfo.device_scale_factor = scale()
        screenInfo.depth = 32
        screenInfo.depth_per_component = 8

        return true
    }

    override fun getScreenPoint(browser: CefBrowser, viewPoint: Point): Point {
        val origin = if (isShowing) locationOnScreen else Point(0, 0)

        return Point(origin.x + viewPoint.x, origin.y + viewPoint.y)
    }

    override fun getDeviceScaleFactor(browser: CefBrowser): Double = scale()

    override fun onPopupShow(browser: CefBrowser, show: Boolean) = Unit

    override fun onPopupSize(browser: CefBrowser, size: Rectangle) = Unit

    override fun onPaint(browser: CefBrowser, popup: Boolean, dirtyRects: Array<out Rectangle>, buffer: ByteBuffer, width: Int, height: Int) {
        if (popup || width <= 0 || height <= 0) {
            return
        }

        synchronized(this) {
            val image = frame?.takeIf { it.width == width && it.height == height } ?: BufferedImage(width, height, BufferedImage.TYPE_INT_ARGB_PRE)
            val pixels = (image.raster.dataBuffer as DataBufferInt).data
            buffer.duplicate().order(ByteOrder.LITTLE_ENDIAN).asIntBuffer().get(pixels, 0, minOf(pixels.size, width * height))
            frame = image
        }

        repaint()
    }

    override fun onCursorChange(browser: CefBrowser, cursorType: Int): Boolean {
        cursor = Cursor.getPredefinedCursor(cursorType.coerceIn(Cursor.DEFAULT_CURSOR, Cursor.MOVE_CURSOR))

        return true
    }

    override fun startDragging(browser: CefBrowser, dragData: CefDragData, mask: Int, x: Int, y: Int): Boolean = false

    override fun updateDragCursor(browser: CefBrowser, operation: Int) = Unit

    private fun scale(): Double = graphicsConfiguration?.defaultTransform?.scaleX ?: 1.0
}
