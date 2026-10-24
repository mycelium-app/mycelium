package mycelium

import mycelium.state.PasteDisplay
import mycelium.state.PastedSegment
import mycelium.state.expandedPastes
import mycelium.state.isLargePaste
import mycelium.state.parsePasted
import mycelium.state.pastedBlock
import mycelium.state.pastedLabel
import mycelium.state.pastedSummary
import mycelium.state.pasteInsertion
import mycelium.state.serialisedPastes
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

class PastedTest {
    private val long = (1..20).joinToString("\n") { "line $it" }

    @Test
    fun aBlockRoundTripsThroughItsSerialisedForm() {
        assertEquals(listOf(PastedSegment.Pasted(long)), parsePasted(pastedBlock(long)))
        assertEquals(20, PastedSegment.Pasted(long).lines)
    }

    @Test
    fun severalBlocksKeepTheTextAroundThem() {
        val message = "before ${pastedBlock("a\nb")} between ${pastedBlock(long)} after"

        assertEquals(
            listOf(
                PastedSegment.Plain("before "),
                PastedSegment.Pasted("a\nb"),
                PastedSegment.Plain(" between "),
                PastedSegment.Pasted(long),
                PastedSegment.Plain(" after"),
            ),
            parsePasted(message),
        )
    }

    @Test
    fun aTrailingNewlineInThePastedTextSurvives() {
        assertEquals(listOf(PastedSegment.Pasted("a\n")), parsePasted(pastedBlock("a\n")))
    }

    @Test
    fun aLiteralTagTypedByAPersonStaysText() {
        assertEquals(listOf(PastedSegment.Plain("use <pasted-text> to wrap")), parsePasted("use <pasted-text> to wrap"))
        assertEquals(listOf(PastedSegment.Plain("<pasted-text lines=\"9\">\nx\n</pasted-text>")), parsePasted("<pasted-text lines=\"9\">\nx\n</pasted-text>"))
        assertEquals(listOf(PastedSegment.Plain("<pasted-text lines=\"1\">\nunclosed")), parsePasted("<pasted-text lines=\"1\">\nunclosed"))
    }

    @Test
    fun onlyPastesOverTheLimitsBecomeBadges() {
        assertFalse(isLargePaste((1..12).joinToString("\n") { "x" }))
        assertTrue(isLargePaste((1..13).joinToString("\n") { "x" }))
        assertFalse(isLargePaste("x".repeat(800)))
        assertTrue(isLargePaste("x".repeat(801)))
    }

    @Test
    fun theInsertionIsFoundInTheMiddleOfExistingText() {
        val before = "ab"
        val after = "a${long}b"

        assertEquals(1 until 1 + long.length, pasteInsertion(before, after))
        assertNull(pasteInsertion("ab", "axb"))
    }

    @Test
    fun markersExpandOrSerialiseAtTheirPosition() {
        val marker = ''
        val pastes = mapOf(marker to long)

        assertEquals("a${long}b", expandedPastes("a${marker}b", pastes))
        assertEquals("a${pastedBlock(long)}b", serialisedPastes("a${marker}b", pastes))
    }

    @Test
    fun theLabelCountsLines() {
        assertEquals("Pasted text · 20 lines", pastedLabel(20))
        assertEquals("Pasted text · 1 line", pastedLabel(1))
    }

    @Test
    fun aSummaryShowsEachPastedBlockAsItsLabel() {
        val message = "look at " + pastedBlock("a\nb\nc") + " and fix it"

        assertEquals("look at [Pasted text · 3 lines] and fix it", pastedSummary(message))
    }

    @Test
    fun aMarkerIsShownAsItsLabelAndTheCursorTreatsItAsOneUnit() {
        val marker = '\uE000'
        val display = PasteDisplay("a${marker}b", mapOf(marker to long))
        val label = pastedLabel(20)

        assertEquals(label, display.labelOf(marker))
        assertEquals(1, display.toShown(1))
        assertEquals(1 + label.length, display.toShown(2))
        assertEquals(1, display.toOriginal(1))
        assertEquals(2, display.toOriginal(1 + label.length / 2))
        assertEquals(2, display.toOriginal(1 + label.length))
    }

    @Test
    fun aMarkerWithoutAHeldPasteIsLeftAsItIs() {
        val display = PasteDisplay("\uE000", emptyMap())

        assertNull(display.labelOf('\uE000'))
        assertEquals(1, display.toShown(1))
    }
}
