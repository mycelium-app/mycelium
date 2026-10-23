package mycelium

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.style.BaselineShift
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import mycelium.ui.inlineMarkup
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private fun render(text: String) = inlineMarkup(text, Color.Black, Color.Blue, null)

class InlineMarkupTest {
    @Test
    fun boldItalicBoldItalicAndStrikethroughStyleTheirInnerText() {
        val text = render("**b** *i* _u_ ***bi*** ~~s~~")

        assertEquals("b i u bi s", text.text)
        assertTrue(text.spanStyles.any { it.item.fontWeight == FontWeight.SemiBold && text.text.substring(it.start, it.end) == "b" })
        assertEquals(listOf("i", "u", "bi"), text.spanStyles.filter { it.item.fontStyle == FontStyle.Italic }.map { text.text.substring(it.start, it.end) })
        assertEquals(listOf("s"), text.spanStyles.filter { it.item.textDecoration == TextDecoration.LineThrough }.map { text.text.substring(it.start, it.end) })
    }

    @Test
    fun arithmeticStarsAndSnakeCaseStayLiteral() {
        assertEquals("2 * 3 * 4 and snake_case_name", render("2 * 3 * 4 and snake_case_name").text)
    }

    @Test
    fun aLinkShowsItsLabelWithCodeInsideAndKeepsItsUrl() {
        val text = render("see [the `docs`](https://example.com/d) now")

        assertEquals("see the docs now", text.text)
        assertEquals(listOf("https://example.com/d"), text.getLinkAnnotations(0, text.length).map { (it.item as LinkAnnotation.Url).url })
    }

    @Test
    fun aBareUrlBecomesALinkWithoutItsTrailingPunctuation() {
        val text = render("Open https://example.com/a?b=1, then <https://example.org/x>.")
        val urls = text.getLinkAnnotations(0, text.length).map { (it.item as LinkAnnotation.Url).url }

        assertEquals(listOf("https://example.com/a?b=1", "https://example.org/x"), urls)
        assertEquals("Open https://example.com/a?b=1, then https://example.org/x.", text.text)
    }

    @Test
    fun anImageShowsItsAltTextAndItsUrlAsALink() {
        val text = render("![diagram](https://example.com/d.png)")

        assertEquals("diagram https://example.com/d.png", text.text)
        assertEquals(1, text.getLinkAnnotations(0, text.length).size)
    }

    @Test
    fun anEscapedMarkerIsShownLiterally() {
        assertEquals("*not italic*", render("\\*not italic\\*").text)
    }

    @Test
    fun superscriptAndSubscriptShiftTheirTextAndASingleTildeIsNotStrikethrough() {
        val text = render("x^2^ H~2~O ~~gone~~")

        assertEquals("x2 H2O gone", text.text)
        assertEquals(listOf("2"), text.spanStyles.filter { it.item.baselineShift == BaselineShift.Superscript }.map { text.text.substring(it.start, it.end) })
        assertEquals(listOf("2"), text.spanStyles.filter { it.item.baselineShift == BaselineShift.Subscript }.map { text.text.substring(it.start, it.end) })
        assertEquals(listOf("gone"), text.spanStyles.filter { it.item.textDecoration == TextDecoration.LineThrough }.map { text.text.substring(it.start, it.end) })
    }

    @Test
    fun carretsAndTildesAroundSpacesStayLiteral() {
        assertEquals("a^b and c^d ~/x and ~/y", render("a^b and c^d ~/x and ~/y").text)
    }

    @Test
    fun aFootnoteReferenceShowsItsLabelRaised() {
        val text = render("Claim[^1].")

        assertEquals("Claim1.", text.text)
        assertEquals(listOf("1"), text.spanStyles.filter { it.item.baselineShift == BaselineShift.Superscript }.map { text.text.substring(it.start, it.end) })
    }
}
