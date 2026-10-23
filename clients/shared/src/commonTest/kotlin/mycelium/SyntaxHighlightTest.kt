package mycelium

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.luminance
import mycelium.state.Token
import mycelium.state.TokenKind
import mycelium.state.highlight
import mycelium.ui.Tokens
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private fun kinds(language: String, code: String) = highlight(language, code).filter { it.kind != TokenKind.Plain }.map { it.kind to it.text }

private fun contrast(a: Color, b: Color): Float {
    val high = maxOf(a.luminance(), b.luminance())
    val low = minOf(a.luminance(), b.luminance())

    return (high + 0.05f) / (low + 0.05f)
}

class SyntaxHighlightTest {
    @Test
    fun kotlinHasKeywordsStringsCommentsAndNumbers() {
        assertEquals(
            listOf(TokenKind.Keyword to "fun", TokenKind.Number to "42", TokenKind.Text to "\"a // b\"", TokenKind.Comment to "// done"),
            kinds("kotlin", "fun x() = 42 + \"a // b\" // done"),
        )
    }

    @Test
    fun rustBlockCommentsSpanLinesAndIdentifiersContainingKeywordsStayPlain() {
        assertEquals(
            listOf(TokenKind.Comment to "/* a\nb */", TokenKind.Keyword to "let"),
            kinds("rust", "/* a\nb */ let letter = other"),
        )
    }

    @Test
    fun pythonAndShellUseHashComments() {
        assertEquals(listOf(TokenKind.Keyword to "def", TokenKind.Comment to "# c"), kinds("py", "def f(): # c"))
        assertEquals(listOf(TokenKind.Keyword to "echo", TokenKind.Text to "'hi'", TokenKind.Comment to "# c"), kinds("console", "echo 'hi' # c"))
    }

    @Test
    fun jsonStringsNumbersAndLiteralsAreMarked() {
        assertEquals(
            listOf(TokenKind.Text to "\"a\"", TokenKind.Number to "1.5", TokenKind.Keyword to "true"),
            kinds("json", "{\"a\": 1.5, \"x\": true}").filter { it.second != "\"x\"" },
        )
    }

    @Test
    fun yamlAndTomlMarkKeysAndSections() {
        assertEquals(listOf(TokenKind.Keyword to "name", TokenKind.Text to "\"x\"", TokenKind.Keyword to "on"), kinds("yaml", "name: \"x\"\n- on: ").filter { it.second != "- " })
        assertEquals(listOf(TokenKind.Keyword to "[package]", TokenKind.Keyword to "version", TokenKind.Text to "\"1\""), kinds("toml", "[package]\nversion = \"1\""))
    }

    @Test
    fun diffMarksAddedRemovedAndHunkLines() {
        assertEquals(
            listOf(TokenKind.Keyword to "@@ -1 +1 @@", TokenKind.Removed to "\n-old", TokenKind.Added to "\n+new"),
            kinds("diff", "@@ -1 +1 @@\n-old\n+new\n context"),
        )
    }

    @Test
    fun anUnknownLanguageStaysOnePlainToken() {
        assertEquals(listOf(Token(TokenKind.Plain, "fun x = 1 // y")), highlight("brainfuck", "fun x = 1 // y"))
    }

    @Test
    fun anUnterminatedStringEndsAtTheLine() {
        assertEquals(listOf(TokenKind.Text to "\"open"), kinds("kotlin", "\"open\nnext"))
    }

    @Test
    fun everyTokenColourMeetsFourPointFiveToOneOnTheCodePanelInBothThemes() {
        for (palette in listOf(Tokens.Dark, Tokens.Light)) {
            val colours = listOf(palette.text, palette.statusWorking, palette.diffAdd, palette.textMuted, palette.statusAttention, palette.diffDel, palette.statusFailed, palette.accent)

            for (colour in colours) {
                assertTrue(contrast(colour, palette.bgPanel) >= 4.5f, "$colour on ${palette.bgPanel}")
            }
        }
    }
}
