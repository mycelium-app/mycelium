package mycelium.desktop

import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toPixelMap
import org.cef.browser.CefBrowser
import java.lang.reflect.Proxy
import java.nio.ByteBuffer
import kotlin.test.assertNotNull
import java.awt.Canvas
import java.awt.Cursor
import java.awt.event.InputEvent
import java.awt.event.MouseEvent
import java.awt.event.MouseWheelEvent
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs

class OffscreenBrowserTest {
    private val window = Canvas()
    private val source = Canvas()

    @Test
    fun theBrowserRendersAtTheFullPhysicalSizeOfItsPanel() {
        assertEquals(1600 to 1200, viewSize(1600, 1200))
        assertEquals(1 to 1, viewSize(0, 0))
    }

    @Test
    fun aScaledAppZoomsThePageByTheSameFactor() {
        assertEquals(0.0, pageZoomLevel(1.0), 1e-9)
        assertEquals(1.0, pageZoomLevel(1.2), 1e-9)
        assertEquals(Math.log(2.0) / Math.log(1.2), pageZoomLevel(2.0), 1e-9)
    }

    @Test
    fun thePagesCursorTypeBecomesTheMatchingPointerIconAndUnknownTypesFallBackToTheDefault() {
        assertEquals(Cursor.E_RESIZE_CURSOR, awtCursorType(Cursor.E_RESIZE_CURSOR))
        assertEquals(Cursor.TEXT_CURSOR, awtCursorType(Cursor.TEXT_CURSOR))
        assertEquals(Cursor.HAND_CURSOR, awtCursorType(Cursor.HAND_CURSOR))
        assertEquals(Cursor.MOVE_CURSOR, awtCursorType(Cursor.MOVE_CURSOR))
        assertEquals(Cursor.DEFAULT_CURSOR, awtCursorType(29))
        assertEquals(Cursor.DEFAULT_CURSOR, awtCursorType(-1))
    }

    @Test
    fun paintedFramesRemainVisibleAndDoNotChangeWhenTheNextFrameArrives() {
        val browser = Proxy.newProxyInstance(CefBrowser::class.java.classLoader, arrayOf(CefBrowser::class.java)) { _, _, _ -> null } as CefBrowser
        val host = OffscreenBrowser()

        host.onPaint(browser, false, emptyArray(), ByteBuffer.wrap(byteArrayOf(0, 0, -1, -1)), 1, 1)
        val first = assertNotNull(host.frame())

        assertEquals(Color.Red, first.toPixelMap()[0, 0])
        host.onPaint(browser, false, emptyArray(), ByteBuffer.wrap(byteArrayOf(-1, 0, 0, -1)), 1, 1)
        val second = assertNotNull(host.frame())

        assertEquals(Color.Blue, second.toPixelMap()[0, 0])
        assertEquals(Color.Red, first.toPixelMap()[0, 0])
    }

    @Test
    fun aClickLandsOnThePhysicalPixelUnderThePointer() {
        val press = MouseEvent(window, MouseEvent.MOUSE_PRESSED, 0, InputEvent.BUTTON1_DOWN_MASK, 900, 700, 1, false, MouseEvent.BUTTON1)

        val forwarded = browserMouseEvent(source, press, Offset(250f, 100f), 2.5)

        assertEquals(MouseEvent.MOUSE_PRESSED, forwarded.id)
        assertEquals(250 to 100, forwarded.x to forwarded.y)
        assertEquals(MouseEvent.BUTTON1, forwarded.button)
    }

    @Test
    fun aWheelTurnDownReachesThePageAsAScrollDownOfAFullNotchAtTheAppsScaleWithItsControlKeyForZooming() {
        val wheel = MouseWheelEvent(window, MouseEvent.MOUSE_WHEEL, 0, InputEvent.CTRL_DOWN_MASK, 10, 10, 0, false, MouseWheelEvent.WHEEL_UNIT_SCROLL, 3, 1)

        val forwarded = assertIs<MouseWheelEvent>(browserMouseEvent(source, wheel, Offset(20f, 20f), 2.0))

        assertEquals(-80, forwarded.wheelRotation)
        assertEquals(true, forwarded.isControlDown)
    }
}
