package mycelium

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.style.BaselineShift
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import mycelium.state.isFilePath
import mycelium.state.parentFolder
import mycelium.ui.FeedBadge
import mycelium.ui.FeedBadgeKind
import mycelium.ui.MyIcon
import mycelium.ui.PathBadges
import mycelium.ui.feedBadgeIcon
import mycelium.ui.parseFeedBadge
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
    fun aBareUrlWithUnderscoresAndAFragmentStaysOneLink() {
        val text = render("Thread: https://github.com/acme-org/backups/pull/4#discussion_r4337728320")
        val urls = text.getLinkAnnotations(0, text.length).map { (it.item as LinkAnnotation.Url).url }

        assertEquals(listOf("https://github.com/acme-org/backups/pull/4#discussion_r4337728320"), urls)
        assertEquals("Thread: https://github.com/acme-org/backups/pull/4#discussion_r4337728320", text.text)
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

    @Test
    fun aFilePathInCodeBecomesAClickableBadgeThatOpensThePath() {
        val opened = mutableListOf<String>()
        val text = inlineMarkup("see `src/main.rs:12` and `x`", Color.Black, Color.Blue, null, null, PathBadges(Color.Gray) { opened += it })

        text.getLinkAnnotations(0, text.length).forEach { (it.item as LinkAnnotation.Clickable).linkInteractionListener?.onClick(it.item) }

        assertEquals(listOf("src/main.rs:12"), opened)
        assertTrue(text.text.endsWith(" and x"))
    }

    @Test
    fun filePathsAreToldApartFromOtherCode() {
        assertTrue(isFilePath("/etc/hosts"))
        assertTrue(isFilePath("src/main.rs:12:3"))
        assertTrue(isFilePath("~/code/app"))
        assertTrue(!isFilePath("a / b"))
        assertTrue(!isFilePath("https://example.com/a.html"))
        assertTrue(!isFilePath("foo.bar"))
        assertEquals("/work/src", parentFolder("/work/src/main.rs:4"))
    }

    @Test
    fun feedBadgesParseEachKindAndCustomBadgesTheirLabelAndIcon() {
        assertEquals(FeedBadge("[[project:Mycelium]]", FeedBadgeKind.Project, "Mycelium"), parseFeedBadge("[[project:Mycelium]]", 0))
        assertEquals(FeedBadge("[[type:release]]", FeedBadgeKind.Type, "release"), parseFeedBadge("[[type:release]]", 0))
        assertEquals(FeedBadge("[[outcome:live]]", FeedBadgeKind.Outcome, "live"), parseFeedBadge("[[outcome:live]]", 0))
        assertEquals(FeedBadge("[[custom:Nightly|moon]]", FeedBadgeKind.Custom, "Nightly", "moon"), parseFeedBadge("[[custom:Nightly|moon]]", 0))
        assertEquals(FeedBadge("[[custom:Beta]]", FeedBadgeKind.Custom, "Beta"), parseFeedBadge("[[custom:Beta]]", 0))
    }

    @Test
    fun feedBadgesUseThePredefinedIconsAndADefaultForAnythingElse() {
        assertEquals(MyIcon.Tag, feedBadgeIcon(FeedBadge("", FeedBadgeKind.Type, "release")))
        assertEquals(MyIcon.Failed, feedBadgeIcon(FeedBadge("", FeedBadgeKind.Outcome, "Failed")))
        assertEquals(MyIcon.Moon, feedBadgeIcon(FeedBadge("", FeedBadgeKind.Custom, "Nightly", "moon")))
        assertEquals(MyIcon.Star, feedBadgeIcon(FeedBadge("", FeedBadgeKind.Custom, "Nightly", "not-an-icon")))
        assertEquals(MyIcon.Star, feedBadgeIcon(FeedBadge("", FeedBadgeKind.Custom, "Nightly")))
    }

    @Test
    fun malformedFeedBadgeMarkupIsNotABadge() {
        for (text in listOf("[[nonsense:label]]", "[[type:]]", "[[type:   ]]", "[[type:release", "[[type]]", "[[type:release|tag]]", "[[custom:Beta|Bad Icon]]", "[[custom:${"x".repeat(33)}]]", "[type:release]")) {
            assertEquals(null, parseFeedBadge(text, 0), text)
        }
    }

    @Test
    fun feedPostMarkupRendersBadgesInlineAndLeavesEverythingElseAsText() {
        val text = inlineMarkup("[[type:release]] v1 is **[[outcome:live]]** and [[nope:x]]", Color.Black, Color.Blue, null, feedBadges = true)

        assertEquals("release v1 is live and [[nope:x]]", text.text)
    }

    @Test
    fun plainPostsAndOtherMarkdownNeverTurnIntoBadges() {
        assertEquals("[[type:release]]", render("[[type:release]]").text)
        assertEquals("Search now finds branches.", inlineMarkup("Search now finds branches.", Color.Black, Color.Blue, null, feedBadges = true).text)
    }
}
