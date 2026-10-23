package mycelium

import mycelium.state.ColumnAlign
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
}
