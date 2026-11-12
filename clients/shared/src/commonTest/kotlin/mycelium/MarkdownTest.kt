package mycelium

import mycelium.state.CalloutKind
import mycelium.state.ColumnAlign
import mycelium.state.Footnote
import mycelium.state.InlineImageRef
import mycelium.state.MarkdownBlock
import mycelium.state.MarkdownListItem
import mycelium.state.markdownBlocks
import kotlin.test.Test
import kotlin.test.assertEquals

private fun item(text: String, checked: Boolean? = null, children: List<MarkdownBlock> = emptyList()) = MarkdownListItem(text, checked, children)

class MarkdownTest {
    @Test
    fun aPipeTableBecomesATableWithShortRowsPaddedAndProseAroundItKept() {
        val text = "Results:\n| Check | Result |\n|---|:---:|\n| sha256 | ok |\n| `latest` |\n\nDone, a | b is prose."

        assertEquals(
            listOf(
                MarkdownBlock.Paragraph("Results:"),
                MarkdownBlock.Table(listOf("Check", "Result"), listOf(listOf("sha256", "ok"), listOf("`latest`", "")), listOf(ColumnAlign.Start, ColumnAlign.Center)),
                MarkdownBlock.Paragraph("Done, a | b is prose."),
            ),
            markdownBlocks(text),
        )
    }

    @Test
    fun headingsListsQuotesRulesAndCodeBecomeTheirOwnBlocks() {
        val text = """
            # Slices

            - No slices in this session.
            - Working tree clean.

            1. Build
            2. Test

            > Quoted
            > twice

            ---

            ```console
            cargo test
            ```
            Plain paragraph
            continues here.
        """.trimIndent()

        assertEquals(
            listOf(
                MarkdownBlock.Heading(1, "Slices"),
                MarkdownBlock.Bullets(listOf(item("No slices in this session."), item("Working tree clean."))),
                MarkdownBlock.Numbered(1, listOf(item("Build"), item("Test"))),
                MarkdownBlock.Quote(listOf(MarkdownBlock.Paragraph("Quoted\ntwice"))),
                MarkdownBlock.Rule,
                MarkdownBlock.Code("console", "cargo test"),
                MarkdownBlock.Paragraph("Plain paragraph\ncontinues here."),
            ),
            markdownBlocks(text),
        )
    }

    @Test
    fun aListDirectlyAfterAParagraphStartsANewBlock() {
        assertEquals(
            listOf(MarkdownBlock.Paragraph("Next:"), MarkdownBlock.Bullets(listOf(item("one")))),
            markdownBlocks("Next:\n- one"),
        )
    }

    @Test
    fun anUnclosedCodeFenceTakesTheRestOfTheMessage() {
        assertEquals(listOf(MarkdownBlock.Code("", "partial\nstream")), markdownBlocks("```\npartial\nstream"))
    }

    @Test
    fun taskListItemsCarryTheirCheckedStateWithoutTheBrackets() {
        assertEquals(
            listOf(MarkdownBlock.Bullets(listOf(item("open", false), item("done", true), item("also done", true), item("[x]not a task")))),
            markdownBlocks("- [ ] open\n- [x] done\n* [X] also done\n- [x]not a task"),
        )
    }

    @Test
    fun indentedItemsNestUnderTheirParentAndContinuationLinesJoinTheItem() {
        val text = "- parent\n  continues here\n  - child one\n  - [x] child two\n- sibling\n\n1. first\n   - inner\n2. second"

        assertEquals(
            listOf(
                MarkdownBlock.Bullets(
                    listOf(
                        item("parent\ncontinues here", children = listOf(MarkdownBlock.Bullets(listOf(item("child one"), item("child two", true))))),
                        item("sibling"),
                    ),
                ),
                MarkdownBlock.Numbered(1, listOf(item("first", children = listOf(MarkdownBlock.Bullets(listOf(item("inner"))))), item("second"))),
            ),
            markdownBlocks(text),
        )
    }

    @Test
    fun anOrderedListKeepsItsStartingNumberAndABlankLineBetweenItemsKeepsOneList() {
        assertEquals(
            listOf(MarkdownBlock.Numbered(3, listOf(item("three"), item("four")))),
            markdownBlocks("3. three\n\n4) four"),
        )
    }

    @Test
    fun aYearAtTheStartOfALineIsNotAListItem() {
        assertEquals(listOf(MarkdownBlock.Paragraph("2024 was long")), markdownBlocks("2024 was long"))
    }

    @Test
    fun headingsKeepLevelsThreeToSix() {
        assertEquals((3..6).map { MarkdownBlock.Heading(it, "H$it") }, markdownBlocks((3..6).joinToString("\n") { "#".repeat(it) + " H$it" }))
    }

    @Test
    fun aQuoteHoldsNestedBlocks() {
        assertEquals(
            listOf(MarkdownBlock.Quote(listOf(MarkdownBlock.Paragraph("note"), MarkdownBlock.Bullets(listOf(item("a"), item("b"))), MarkdownBlock.Quote(listOf(MarkdownBlock.Paragraph("deeper")))))),
            markdownBlocks("> note\n>\n> - a\n> - b\n>\n> > deeper"),
        )
    }

    @Test
    fun tableColumnsReadTheirAlignmentFromTheDivider() {
        val table = markdownBlocks("| a | b | c | d |\n|---|:--|:-:|--:|\n| 1 | 2 | 3 | 4 |").single() as MarkdownBlock.Table

        assertEquals(listOf(ColumnAlign.Start, ColumnAlign.Start, ColumnAlign.Center, ColumnAlign.End), table.aligns)
    }

    @Test
    fun aCodeFenceInsideAListItemStaysInThatItem() {
        assertEquals(
            listOf(MarkdownBlock.Bullets(listOf(item("run", children = listOf(MarkdownBlock.Code("sh", "make")))))),
            markdownBlocks("- run\n  ```sh\n  make\n  ```"),
        )
    }

    @Test
    fun aQuoteStartingWithAnAlertMarkerBecomesACalloutOfThatKind() {
        assertEquals(
            listOf(MarkdownBlock.Callout(CalloutKind.Warning, listOf(MarkdownBlock.Paragraph("Back up first.")))),
            markdownBlocks("> [!WARNING]\n> Back up first."),
        )
        assertEquals(CalloutKind.Tip, (markdownBlocks("> [!tip]\n> x").single() as MarkdownBlock.Callout).kind)
    }

    @Test
    fun anUnknownAlertMarkerStaysAnOrdinaryQuote() {
        assertEquals(listOf(MarkdownBlock.Quote(listOf(MarkdownBlock.Paragraph("[!NOPE]\ntext")))), markdownBlocks("> [!NOPE]\n> text"))
    }

    @Test
    fun footnoteDefinitionsLeaveTheTextAndGatherAtTheEnd() {
        assertEquals(
            listOf(
                MarkdownBlock.Paragraph("Claim[^1] and more[^note]."),
                MarkdownBlock.Footnotes(listOf(Footnote("1", "First source."), Footnote("note", "Second."))),
            ),
            markdownBlocks("Claim[^1] and more[^note].\n\n[^1]: First source.\n[^note]: Second.\n"),
        )
    }

    private fun ref(alt: String, host: String, name: String, page: String? = null) = InlineImageRef(alt, "https://$host/$name.png", host, page)

    @Test
    fun imagesOnConsecutiveLinesFormOneGalleryWithTheirPageUrls() {
        val text = "Four options:\n\n![One](https://a.example.org/1.png \"https://shop.example.com/1\")\n![Two](https://a.example.org/2.png)\n![Three](https://b.example.org/3.png)\n![Four](https://b.example.org/4.png)\n\nTell me the weight."

        assertEquals(
            listOf(
                MarkdownBlock.Paragraph("Four options:"),
                MarkdownBlock.Images(
                    listOf(
                        ref("One", "a.example.org", "1", "https://shop.example.com/1"),
                        ref("Two", "a.example.org", "2"),
                        ref("Three", "b.example.org", "3"),
                        ref("Four", "b.example.org", "4"),
                    ),
                ),
                MarkdownBlock.Paragraph("Tell me the weight."),
            ),
            markdownBlocks(text),
        )
    }

    @Test
    fun anImageInsideAListItemIsLiftedOutAfterItsText() {
        val blocks = markdownBlocks("- Strips ![Strips](https://a.example.org/s.png) hold 7 kg\n- Hooks")

        assertEquals(
            listOf(
                MarkdownBlock.Bullets(
                    listOf(
                        item("Strips hold 7 kg", children = listOf(MarkdownBlock.Images(listOf(ref("Strips", "a.example.org", "s"))))),
                        item("Hooks"),
                    ),
                ),
            ),
            blocks,
        )
    }

    @Test
    fun anImageInsideAParagraphIsLiftedOutAfterThatParagraphsText() {
        assertEquals(
            listOf(MarkdownBlock.Paragraph("This one: it is brass."), MarkdownBlock.Images(listOf(ref("Hook", "a.example.org", "h")))),
            markdownBlocks("This one: ![Hook](https://a.example.org/h.png) it is brass."),
        )
    }

    @Test
    fun anImageWithNoAltTextStaysALink() {
        assertEquals(listOf(MarkdownBlock.Paragraph("![](https://a.example.org/h.png)")), markdownBlocks("![](https://a.example.org/h.png)"))
    }

    @Test
    fun anImageOverPlainHttpStaysALink() {
        assertEquals(listOf(MarkdownBlock.Paragraph("![Hook](http://a.example.org/h.png)")), markdownBlocks("![Hook](http://a.example.org/h.png)"))
    }

    @Test
    fun theHostOmitsThePortAndUserInfo() {
        val image = (markdownBlocks("![Hook](https://me@a.example.org:8443/h.png?x=1)").single() as MarkdownBlock.Images).images.single()

        assertEquals("a.example.org", image.host)
        assertEquals("https://me@a.example.org:8443/h.png?x=1", image.url)
    }
}
