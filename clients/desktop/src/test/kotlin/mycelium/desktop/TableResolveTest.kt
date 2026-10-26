package mycelium.desktop

import mycelium.ui.resolveTable
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class TableResolveTest {
    private val min = listOf(100, 80, 60)
    private val natural = listOf(300, 120, 70)

    @Test
    fun sumEqualsAvailableWhenMinimumsFit() {
        for (available in listOf(240, 300, 400, 490, 900, 1600)) {
            val resolved = resolveTable(min, natural, listOf(1f, 0f, 0f), available)

            assertEquals(available, resolved.widths.sum(), "available $available")
            assertEquals(available, resolved.total)
            assertFalse(resolved.scroll)
        }
    }

    @Test
    fun noColumnBelowMin() {
        for (available in listOf(0, 100, 240, 300, 400, 900)) {
            val resolved = resolveTable(min, natural, listOf(1f, 0f, 0f), available)

            resolved.widths.forEachIndexed { index, width -> assertTrue(width >= min[index], "column $index at $available") }
        }
    }

    @Test
    fun overflowSetsScrollAndTotalIsSumOfMinimums() {
        val resolved = resolveTable(min, natural, listOf(1f, 0f, 0f), 200)

        assertTrue(resolved.scroll)
        assertEquals(240, resolved.total)
        assertEquals(min, resolved.widths)
    }

    @Test
    fun twoWeightedColumnsShareSpareByWeight() {
        val resolved = resolveTable(listOf(10, 10), listOf(100, 100), listOf(1f, 3f), 600)

        assertEquals(listOf(200, 400), resolved.widths)
    }

    @Test
    fun shortfallTakenFromWeightedFirst() {
        val resolved = resolveTable(min, natural, listOf(1f, 0f, 0f), 400)

        assertEquals(listOf(210, 120, 70), resolved.widths)
    }

    @Test
    fun spareGoesToTheFirstColumnWhenNothingIsWeighted() {
        val resolved = resolveTable(min, natural, listOf(0f, 0f, 0f), 600)

        assertEquals(listOf(410, 120, 70), resolved.widths)
    }

    @Test
    fun tableHugsContentWhenNotFilling() {
        val resolved = resolveTable(min, natural, listOf(1f, 0f, 0f), 900, fill = false)

        assertEquals(natural, resolved.widths)
        assertEquals(490, resolved.total)
    }

    @Test
    fun deterministic() {
        assertEquals(resolveTable(min, natural, listOf(1f, 2f, 0f), 333), resolveTable(min, natural, listOf(1f, 2f, 0f), 333))
    }
}
