package mycelium

import mycelium.state.MarkdownBlock
import mycelium.state.markdownBlocks
import kotlin.test.Test
import kotlin.test.assertEquals

class MarkdownTest {
    @Test
    fun aPipeTableBecomesATableWithShortRowsPaddedAndProseAroundItKept() {
        val text = "Results:\n| Check | Result |\n|---|:---:|\n| sha256 | ok |\n| `latest` |\n\nDone, a | b is prose."

        assertEquals(
            listOf(
                MarkdownBlock.Paragraph("Results:"),
                MarkdownBlock.Table(listOf("Check", "Result"), listOf(listOf("sha256", "ok"), listOf("`latest`", ""))),
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
                MarkdownBlock.Bullets(listOf("No slices in this session.", "Working tree clean.")),
                MarkdownBlock.Numbered(1, listOf("Build", "Test")),
                MarkdownBlock.Quote("Quoted\ntwice"),
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
            listOf(MarkdownBlock.Paragraph("Next:"), MarkdownBlock.Bullets(listOf("one"))),
            markdownBlocks("Next:\n- one"),
        )
    }

    @Test
    fun anUnclosedCodeFenceTakesTheRestOfTheMessage() {
        assertEquals(listOf(MarkdownBlock.Code("", "partial\nstream")), markdownBlocks("```\npartial\nstream"))
    }
}
