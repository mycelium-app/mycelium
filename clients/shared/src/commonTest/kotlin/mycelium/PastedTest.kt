package mycelium

import mycelium.state.BadgeGap
import mycelium.state.MarkedText
import mycelium.state.PasteDisplay
import mycelium.state.PastedSegment
import mycelium.state.expandedPastes
import mycelium.state.isLargePaste
import mycelium.state.imageLabel
import mycelium.state.orphanedImageIds
import mycelium.state.parsePasted
import mycelium.state.pastedBlock
import mycelium.state.pastedLabel
import mycelium.state.pastedSummary
import mycelium.state.pasteInsertion
import mycelium.state.serialisedPastes
import mycelium.state.withImageMarkers
import mycelium.state.withoutImageMarkers
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

        val width = label.length + 2 * BadgeGap.length

        assertEquals(label, display.labelOf(marker))
        assertEquals(1, display.toShown(1))
        assertEquals(1 + width, display.toShown(2))
        assertEquals(1, display.toOriginal(1))
        assertEquals(2, display.toOriginal(1 + width / 2))
        assertEquals(2, display.toOriginal(1 + width))
        assertEquals(listOf((1 + BadgeGap.length) until (1 + BadgeGap.length + label.length)), display.badgeRanges().values.toList())
    }

    @Test
    fun theCursorLandsOnTheNearerEdgeOfAMarkerSoMovingUpOrDownNeverSticksAfterIt() {
        val marker = '\uE000'
        val display = PasteDisplay("a${marker}b", mapOf(marker to long))
        val width = pastedLabel(20).length + 2 * BadgeGap.length

        assertEquals(1, display.toOriginal(2))
        assertEquals(1, display.toOriginal(1 + width / 2 - 1))
        assertEquals(2, display.toOriginal(1 + width / 2))
        assertEquals(2, display.toOriginal(width))
    }

    @Test
    fun anImageMarkerSurvivesExpandingPastesForTheDraft() {
        val image = '\uE001'
        val paste = '\uE000'

        assertEquals("a${image}b$long", expandedPastes("a${image}b$paste", mapOf(paste to long)))
    }

    @Test
    fun aMarkerWithoutAHeldPasteIsLeftAsItIs() {
        val display = PasteDisplay("\uE000", emptyMap())

        assertNull(display.labelOf('\uE000'))
        assertEquals(1, display.toShown(1))
    }

    @Test
    fun anImageMarkerIsShownAsItsFileName() {
        val marker = '\uE001'
        val display = PasteDisplay("a${marker}", emptyMap(), mapOf(marker to "shot.png"))

        assertEquals("Image · shot.png", display.labelOf(marker))
        assertEquals(imageLabel("shot.png"), display.labelOf(marker))
    }

    @Test
    fun imageMarkersAreInsertedAtTheCursorInAttachOrder() {
        val marked = withImageMarkers(MarkedText("ab", 1, emptyMap()), listOf(7, 9), emptySet())

        assertEquals(4, marked.text.length)
        assertEquals(3, marked.cursor)
        assertEquals(listOf(7, 9), marked.text.filter { it in marked.images }.map { marked.images.getValue(it) })
        assertEquals("a", marked.text.take(1))
        assertEquals("b", marked.text.last().toString())
    }

    @Test
    fun imageMarkersNeverReuseAPasteMarker() {
        val marked = withImageMarkers(MarkedText("", 0, emptyMap()), listOf(1), setOf('\uE000'))

        assertEquals(mapOf('\uE001' to 1), marked.images)
    }

    @Test
    fun removingAnImageTakesItsMarkerOutAndKeepsTheCursorAfterTheSameText() {
        val marker = '\uE000'
        val cleaned = withoutImageMarkers(MarkedText("a${marker}b", 3, mapOf(marker to 4)), setOf(4))

        assertEquals(MarkedText("ab", 2, emptyMap()), cleaned)
    }

    @Test
    fun anImageWhoseMarkerWasDeletedIsOrphaned() {
        val kept = '\uE000'
        val deleted = '\uE001'

        assertEquals(listOf(2), orphanedImageIds("a${kept}", mapOf(kept to 1, deleted to 2)))
    }
}
