package mycelium

import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import mycelium.ui.ViewTransform
import mycelium.ui.clampOffset
import mycelium.ui.clampScale
import mycelium.ui.fitSize
import mycelium.ui.imageBounds
import mycelium.ui.panBy
import mycelium.ui.stepIndex
import mycelium.ui.toggleZoom
import mycelium.ui.zoomAbout
import mycelium.ui.zoomPercent
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class ImageViewerTest {
    private val window = Size(1000f, 800f)
    private val fitted = fitSize(Size(2000f, 1000f), window)

    @Test
    fun aWideImageIsFittedToTheWindowWidthKeepingItsAspectRatio() {
        assertEquals(Size(1000f, 500f), fitted)
    }

    @Test
    fun aTallImageIsFittedToTheWindowHeight() {
        assertEquals(Size(400f, 800f), fitSize(Size(500f, 1000f), window))
    }

    @Test
    fun anImageWithNoSizeFitsToNothing() {
        assertEquals(Size.Zero, fitSize(Size.Zero, window))
    }

    @Test
    fun scaleIsClampedBetweenFitAndEightTimes() {
        assertEquals(1f, clampScale(0.3f))
        assertEquals(3f, clampScale(3f))
        assertEquals(8f, clampScale(20f))
    }

    @Test
    fun anImageThatFitsCannotBePannedAtFitScale() {
        assertEquals(Offset.Zero, clampOffset(Offset(300f, -200f), 1f, fitted, window))
    }

    @Test
    fun aZoomedImageCannotBePannedPastItsEdges() {
        val clamped = clampOffset(Offset(5000f, -5000f), 4f, fitted, window)

        assertEquals(Offset(1500f, -600f), clamped)
    }

    @Test
    fun panningMovesAZoomedImageByTheDragWithinTheLimits() {
        val zoomed = ViewTransform(2f)

        assertEquals(Offset(40f, 30f), panBy(zoomed, Offset(40f, 30f), fitted, window).offset)
        assertEquals(Offset(500f, 0f), panBy(zoomed, Offset(9000f, 0f), fitted, window).offset)
    }

    @Test
    fun zoomingAboutThePointerKeepsThePointUnderThePointerFixed() {
        val anchor = Offset(100f, 50f)
        val before = ViewTransform(2f, Offset(-200f, 0f))
        val after = zoomAbout(before, 4f, anchor, fitted, window)
        val pointBefore = (anchor - before.offset) / before.scale
        val pointAfter = (anchor - after.offset) / after.scale

        assertEquals(pointBefore, pointAfter)
    }

    @Test
    fun zoomingOutToFitRecentresTheImage() {
        val zoomed = ViewTransform(3f, Offset(400f, 100f))

        assertEquals(ViewTransform(), zoomAbout(zoomed, 0.5f, Offset(300f, 200f), fitted, window))
    }

    @Test
    fun zoomingPastTheMaximumStopsAtEightTimes() {
        assertEquals(8f, zoomAbout(ViewTransform(), 50f, Offset.Zero, fitted, window).scale)
    }

    @Test
    fun doubleTappingAtFitZoomsToTwoAndAHalfTimesAndAgainReturnsToFit() {
        val zoomed = toggleZoom(ViewTransform(), Offset(100f, 0f), fitted, window)

        assertEquals(2.5f, zoomed.scale)
        assertEquals(ViewTransform(), toggleZoom(zoomed, Offset(100f, 0f), fitted, window))
    }

    @Test
    fun theImageBoundsGrowWithZoomSoTapsOutsideItCanBeTold() {
        assertTrue(imageBounds(ViewTransform(), fitted).contains(Offset(400f, 200f)))
        assertFalse(imageBounds(ViewTransform(), fitted).contains(Offset(0f, 350f)))
        assertTrue(imageBounds(ViewTransform(2f), fitted).contains(Offset(0f, 350f)))
    }

    @Test
    fun steppingStopsAtTheFirstAndLastImage() {
        assertEquals(1, stepIndex(0, 1, 3))
        assertEquals(0, stepIndex(0, -1, 3))
        assertEquals(2, stepIndex(2, 1, 3))
    }

    @Test
    fun theZoomReadoutIsAWholePercentage() {
        assertEquals("100%", zoomPercent(1f))
        assertEquals("250%", zoomPercent(2.5f))
    }
}
