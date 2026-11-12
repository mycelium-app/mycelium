package mycelium.desktop

import java.awt.event.KeyEvent
import kotlin.test.Test
import kotlin.test.assertEquals

class ZoomTest {
    private fun zoomOutFrom(start: Float): List<Float> = generateSequence(start) { nextScale(it, KeyEvent.VK_MINUS) }.drop(1).toList()

    @Test
    fun zoomingOutFromTheDefaultStepsDownToHalfSizeLikePlatformDesktop() {
        assertEquals(listOf(0.9f, 0.8f, 0.75f, 0.67f, 0.6f, 0.5f), zoomOutFrom(1f))
    }

    @Test
    fun zoomingInFromHalfSizeClimbsBackThroughEveryStepAndResetGoesToFullSize() {
        assertEquals(0.6f, nextScale(0.5f, KeyEvent.VK_EQUALS))
        assertEquals(1f, nextScale(0.5f, KeyEvent.VK_0))
    }
}
