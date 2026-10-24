package mycelium.desktop

import androidx.compose.ui.geometry.Offset
import java.awt.Canvas
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
    fun theBrowserLaysOutAtTheAppsScaleSoAScaledAppGetsAScaledPage() {
        assertEquals(800 to 600, viewSize(1600, 1200, 2.0))
        assertEquals(640 to 480, viewSize(1600, 1200, 2.5))
    }

    @Test
    fun aClickLandsWhereThePointerIsInThePagesOwnCoordinates() {
        val press = MouseEvent(window, MouseEvent.MOUSE_PRESSED, 0, InputEvent.BUTTON1_DOWN_MASK, 900, 700, 1, false, MouseEvent.BUTTON1)

        val forwarded = browserMouseEvent(source, press, Offset(250f, 100f), 2.5)

        assertEquals(MouseEvent.MOUSE_PRESSED, forwarded.id)
        assertEquals(100 to 40, forwarded.x to forwarded.y)
        assertEquals(MouseEvent.BUTTON1, forwarded.button)
    }

    @Test
    fun aWheelTurnDownReachesThePageAsAScrollDownOfAFullNotchWithItsControlKeyForZooming() {
        val wheel = MouseWheelEvent(window, MouseEvent.MOUSE_WHEEL, 0, InputEvent.CTRL_DOWN_MASK, 10, 10, 0, false, MouseWheelEvent.WHEEL_UNIT_SCROLL, 3, 1)

        val forwarded = assertIs<MouseWheelEvent>(browserMouseEvent(source, wheel, Offset(20f, 20f), 2.0))

        assertEquals(-40, forwarded.wheelRotation)
        assertEquals(true, forwarded.isControlDown)
    }
}
