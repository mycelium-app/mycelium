package mycelium

import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntRect
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.LayoutDirection
import mycelium.ui.BelowAnchor
import kotlin.test.Test
import kotlin.test.assertEquals

class PopupPlacementTest {
    private val window = IntSize(1000, 800)
    private val popup = IntSize(400, 300)

    private fun place(anchor: IntRect) = BelowAnchor(4).calculatePosition(anchor, window, LayoutDirection.Ltr, popup)

    @Test
    fun opensBelowTheAnchorAlignedToItsStart() {
        assertEquals(IntOffset(100, 144), place(IntRect(100, 100, 200, 140)))
    }

    @Test
    fun alignsToTheAnchorsEndWhenTheWindowRunsOutOnTheRight() {
        assertEquals(IntOffset(560, 44), place(IntRect(800, 0, 960, 40)))
    }

    @Test
    fun opensAboveWhenThereIsNoRoomBelow() {
        assertEquals(IntOffset(100, 396), place(IntRect(100, 700, 200, 740)))
    }
}
