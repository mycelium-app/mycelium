package mycelium

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.font.FontStyle
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
}
