package mycelium

import mycelium.state.CellStyle
import mycelium.state.Terminal
import mycelium.state.TerminalColor
import mycelium.state.TerminalKey
import mycelium.state.terminalControlInput
import mycelium.state.terminalKeyInput
import mycelium.state.xtermRgb
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

private const val Esc = "\u001B"

private fun terminal(columns: Int = 20, rows: Int = 5, output: String = ""): Terminal = Terminal(columns, rows).also { it.feed(output) }

class TerminalTest {
    @Test
    fun printableTextAndLineEndingsFillTheGrid() {
        val screen = terminal(output = "hello\r\nworld")

        assertEquals("hello\nworld", screen.screenText().trimEnd('\n'))
        assertEquals(1 to 5, screen.cursorRow to screen.cursorColumn)
    }

    @Test
    fun linefeedAloneKeepsTheColumnAndCarriageReturnReturnsToIt() {
        val screen = terminal(output = "ab\ncd\rX")

        assertEquals("ab", screen.lineText(0))
        assertEquals("X cd", screen.lineText(1))
        assertEquals('X'.code, screen.cell(1, 0).codePoint)
    }

    @Test
    fun backspaceMovesBackAndOverwrites() {
        val screen = terminal(output = "abc\b\bX")

        assertEquals("aXc", screen.lineText(0))
    }

    @Test
    fun tabAdvancesToTheNextStopOfEight() {
        val screen = terminal(output = "a\tb")

        assertEquals('b'.code, screen.cell(0, 8).codePoint)
    }

    @Test
    fun theBellAndUnknownControlsAreIgnored() {
        val screen = terminal(output = "a\u0007b\u0000c")

        assertEquals("abc", screen.lineText(0))
    }

    @Test
    fun clearScreenAndHomeAsTmuxSendsThemEmptyTheGridAndHomeTheCursor() {
        val screen = terminal(output = "one\r\ntwo\r\n${Esc}[H${Esc}[2J")

        assertEquals("", screen.screenText().trim())
        assertEquals(0 to 0, screen.cursorRow to screen.cursorColumn)
    }

    @Test
    fun cursorMovesAreRelativeAbsoluteAndClampedToTheScreen() {
        val screen = terminal(columns = 10, rows = 5)

        screen.feed("${Esc}[3;4H")
        assertEquals(2 to 3, screen.cursorRow to screen.cursorColumn)

        screen.feed("${Esc}[A${Esc}[2C")
        assertEquals(1 to 5, screen.cursorRow to screen.cursorColumn)

        screen.feed("${Esc}[2B${Esc}[3D")
        assertEquals(3 to 2, screen.cursorRow to screen.cursorColumn)

        screen.feed("${Esc}[E")
        assertEquals(4 to 0, screen.cursorRow to screen.cursorColumn)

        screen.feed("${Esc}[2F")
        assertEquals(2 to 0, screen.cursorRow to screen.cursorColumn)

        screen.feed("${Esc}[7G${Esc}[5d")
        assertEquals(4 to 6, screen.cursorRow to screen.cursorColumn)

        screen.feed("${Esc}[99;99f")
        assertEquals(4 to 9, screen.cursorRow to screen.cursorColumn)

        screen.feed("${Esc}[99A${Esc}[99D")
        assertEquals(0 to 0, screen.cursorRow to screen.cursorColumn)
    }

    @Test
    fun eraseInLineHonoursItsThreeModes() {
        val line = "0123456789"

        assertEquals("01234", terminal(output = "$line${Esc}[1;6H${Esc}[K").lineText(0))
        assertEquals("     56789", terminal(output = "$line${Esc}[1;5H${Esc}[1K").lineText(0))
        assertEquals("", terminal(output = "$line${Esc}[1;5H${Esc}[2K").lineText(0))
    }

    @Test
    fun eraseInDisplayHonoursItsModes() {
        val rows = "aaa\r\nbbb\r\nccc"

        assertEquals("aaa\nb\n", terminal(columns = 5, rows = 3, output = "$rows${Esc}[2;2H${Esc}[J").screenText())
        assertEquals("\n  b\nccc", terminal(columns = 5, rows = 3, output = "$rows${Esc}[2;2H${Esc}[1J").screenText())
        assertEquals("\n\n", terminal(columns = 5, rows = 3, output = "$rows${Esc}[2J").screenText())
    }

    @Test
    fun longLinesWrapOntoTheNextRow() {
        val screen = terminal(columns = 5, rows = 3, output = "abcdefgh")

        assertEquals("abcde", screen.lineText(0))
        assertEquals("fgh", screen.lineText(1))
    }

    @Test
    fun aLineThatExactlyFillsTheRowWaitsForTheNextCharacterBeforeWrapping() {
        val screen = terminal(columns = 5, rows = 3, output = "abcde\r\nfg")

        assertEquals("abcde", screen.lineText(0))
        assertEquals("fg", screen.lineText(1))
        assertEquals("", screen.lineText(2))
    }

    @Test
    fun outputPastTheLastRowScrollsTheScreenUp() {
        val screen = terminal(columns = 5, rows = 3, output = "1\r\n2\r\n3\r\n4\r\n5")

        assertEquals("3\n4\n5", screen.screenText())
    }

    @Test
    fun aScrollRegionScrollsOnlyItsOwnRows() {
        val screen = terminal(columns = 6, rows = 5, output = "top\r\na\r\nb\r\nc\r\nstatus${Esc}[2;4r${Esc}[4;1H\nd")

        assertEquals("top\nb\nc\nd\nstatus", screen.screenText())
    }

    @Test
    fun settingTheScrollRegionHomesTheCursor() {
        val screen = terminal(output = "abc${Esc}[2;4r")

        assertEquals(0 to 0, screen.cursorRow to screen.cursorColumn)
    }

    @Test
    fun insertAndDeleteLinesShiftRowsWithinTheRegion() {
        val rows = "a\r\nb\r\nc\r\nd"

        assertEquals("a\n\nb\nc", terminal(columns = 3, rows = 4, output = "$rows${Esc}[2;1H${Esc}[L").screenText())
        assertEquals("a\nc\nd\n", terminal(columns = 3, rows = 4, output = "$rows${Esc}[2;1H${Esc}[M").screenText())
    }

    @Test
    fun insertAndDeleteCharactersShiftTheRestOfTheLine() {
        assertEquals("ab  cdef", terminal(columns = 8, output = "abcdef${Esc}[1;3H${Esc}[2@").lineText(0))
        assertEquals("abef", terminal(columns = 8, output = "abcdef${Esc}[1;3H${Esc}[2P").lineText(0))
        assertEquals("ab  ef", terminal(columns = 8, output = "abcdef${Esc}[1;3H${Esc}[2X").lineText(0))
    }

    @Test
    fun theAlternateScreenKeepsTheMainScreenAndCursorUntilItIsLeft() {
        val screen = terminal(columns = 20, rows = 4, output = "prompt$ ls\r\nfile")

        screen.feed("${Esc}[?1049h${Esc}[22;0;0t${Esc}[H${Esc}[2J${Esc}[?25l")
        assertTrue(screen.alternateScreen)
        assertFalse(screen.cursorVisible)
        assertEquals("", screen.screenText().trim())

        screen.feed("${Esc}[4;1H${Esc}[30m${Esc}[42m[mycelium-0:bash*${Esc}(B${Esc}[m")
        assertEquals("[mycelium-0:bash*", screen.lineText(3))
        assertEquals(TerminalColor.Indexed(2), screen.cell(3, 0).style.background)

        screen.feed("${Esc}[?25h${Esc}[?1049l")
        assertFalse(screen.alternateScreen)
        assertTrue(screen.cursorVisible)
        assertEquals("prompt$ ls\nfile\n\n", screen.screenText())
        assertEquals(1 to 4, screen.cursorRow to screen.cursorColumn)
    }

    @Test
    fun theOlderAlternateScreenModeSwitchesWithoutClearing() {
        val screen = terminal(output = "main")

        screen.feed("${Esc}[?47hother")
        assertEquals("other", screen.lineText(0).trim())

        screen.feed("${Esc}[?47l")
        assertEquals("main", screen.lineText(0))
    }

    @Test
    fun sixteenColoursSelectIndexedForegroundAndBackground() {
        val screen = terminal(output = "${Esc}[31mr${Esc}[1;92mg${Esc}[44mb${Esc}[0mn")

        assertEquals(TerminalColor.Indexed(1), screen.cell(0, 0).style.foreground)
        assertEquals(CellStyle(foreground = TerminalColor.Indexed(10), bold = true), screen.cell(0, 1).style)
        assertEquals(TerminalColor.Indexed(4), screen.cell(0, 2).style.background)
        assertEquals(CellStyle(), screen.cell(0, 3).style)
    }

    @Test
    fun colourCubeAndTruecolourAreSupportedInBothSemicolonAndColonForms() {
        val screen = terminal(output = "${Esc}[38;5;196ma${Esc}[48;2;10;20;30mb${Esc}[38:2:1:2:3mc${Esc}[39;49md")

        assertEquals(TerminalColor.Indexed(196), screen.cell(0, 0).style.foreground)
        assertEquals(TerminalColor.Rgb(10, 20, 30), screen.cell(0, 1).style.background)
        assertEquals(TerminalColor.Default, screen.cell(0, 3).style.foreground)
        assertEquals(TerminalColor.Default, screen.cell(0, 3).style.background)
    }

    @Test
    fun attributesTurnOnAndOffIndividually() {
        val screen = terminal(output = "${Esc}[1;2;3;4;7ma${Esc}[22;23;24;27mb${Esc}mc")

        assertEquals(CellStyle(bold = true, dim = true, italic = true, underline = true, inverse = true), screen.cell(0, 0).style)
        assertEquals(CellStyle(), screen.cell(0, 1).style)
        assertEquals(CellStyle(), screen.cell(0, 2).style)
    }

    @Test
    fun erasedCellsKeepTheCurrentBackgroundOnly() {
        val screen = terminal(output = "${Esc}[1;41m${Esc}[2K")

        assertEquals(CellStyle(background = TerminalColor.Indexed(1)), screen.cell(0, 3).style)
    }

    @Test
    fun utf8CharactersAreDecodedEvenWhenSplitAcrossChunks() {
        val screen = terminal()
        val bytes = "aé€😀".encodeToByteArray()

        screen.feed(bytes.copyOfRange(0, 2))
        screen.feed(bytes.copyOfRange(2, 5))
        screen.feed(bytes.copyOfRange(5, bytes.size))

        assertEquals("aé€😀", screen.lineText(0))
        assertEquals(0x1F600, screen.cell(0, 3).codePoint)
    }

    @Test
    fun invalidUtf8BecomesAReplacementCharacter() {
        val screen = terminal()

        screen.feed(byteArrayOf(0xE2.toByte(), 'x'.code.toByte()))

        assertEquals("�x", screen.lineText(0))
    }

    @Test
    fun oscStringsCharsetSelectionsAndUnknownSequencesAreSkipped() {
        val screen = terminal(output = "a${Esc}]0;title\u0007b${Esc}]10;?${Esc}\\c${Esc}(Bd${Esc}[?2004h${Esc}[>qe${Esc}P1\$r0m${Esc}\\f${Esc}[5 qg${Esc}[999zh${Esc}=${Esc}>i")

        assertEquals("abcdefghi", screen.lineText(0))
    }

    @Test
    fun applicationCursorKeysFollowTheDecPrivateMode() {
        val screen = terminal()

        screen.feed("${Esc}[?1h")
        assertTrue(screen.applicationCursorKeys)

        screen.feed("${Esc}[?1l")
        assertFalse(screen.applicationCursorKeys)
    }

    @Test
    fun saveAndRestoreCursorReturnToTheSavedPositionAndStyle() {
        val screen = terminal(output = "${Esc}[2;3H${Esc}[31m${Esc}7${Esc}[1;1H${Esc}[0m${Esc}8x")

        assertEquals(TerminalColor.Indexed(1), screen.cell(1, 2).style.foreground)
        assertEquals('x'.code, screen.cell(1, 2).codePoint)
    }

    @Test
    fun reverseIndexAtTheTopScrollsDown() {
        val screen = terminal(columns = 5, rows = 3, output = "a\r\nb\r\nc${Esc}[H${Esc}M")

        assertEquals("\na\nb", screen.screenText())
    }

    @Test
    fun resizingKeepsTheContentThatStillFitsAndClampsTheCursor() {
        val screen = terminal(columns = 10, rows = 4, output = "abcdef\r\nsecond${Esc}[4;9H")

        screen.resize(4, 2)

        assertEquals("abcd\nseco", screen.screenText())
        assertEquals(1 to 3, screen.cursorRow to screen.cursorColumn)
    }

    @Test
    fun aMissingParameterDefaultsSoBareSequencesWork() {
        val screen = terminal(output = "abc${Esc}[D${Esc}[0K${Esc}[;3H")

        assertEquals("ab", screen.lineText(0))
        assertEquals(0 to 2, screen.cursorRow to screen.cursorColumn)
    }

    @Test
    fun keysMapToTheBytesATerminalExpects() {
        assertEquals("\r", terminalKeyInput(TerminalKey.Enter))
        assertEquals("\u007F", terminalKeyInput(TerminalKey.Backspace))
        assertEquals("\t", terminalKeyInput(TerminalKey.Tab))
        assertEquals("\u001B", terminalKeyInput(TerminalKey.Escape))
        assertEquals("\u001B[A", terminalKeyInput(TerminalKey.Up))
        assertEquals("\u001B[D", terminalKeyInput(TerminalKey.Left))
        assertEquals("\u001B[H", terminalKeyInput(TerminalKey.Home))
        assertEquals("\u001B[F", terminalKeyInput(TerminalKey.End))
        assertEquals("\u001B[5~", terminalKeyInput(TerminalKey.PageUp))
        assertEquals("\u001B[6~", terminalKeyInput(TerminalKey.PageDown))
        assertEquals("\u001B[3~", terminalKeyInput(TerminalKey.Delete))
    }

    @Test
    fun arrowsUseTheApplicationFormWhenTheProgramAsksForIt() {
        assertEquals("\u001BOA", terminalKeyInput(TerminalKey.Up, applicationCursorKeys = true))
        assertEquals("\u001BOC", terminalKeyInput(TerminalKey.Right, applicationCursorKeys = true))
    }

    @Test
    fun controlLettersMapToTheirControlCodes() {
        assertEquals("\u0003", terminalControlInput('c'))
        assertEquals("\u0004", terminalControlInput('D'))
        assertEquals("\u001A", terminalControlInput('z'))
        assertNull(terminalControlInput('1'))
    }

    @Test
    fun theExtendedPaletteFollowsTheXtermCubeAndGreyRamp() {
        assertEquals(Triple(0, 0, 0), xtermRgb(16))
        assertEquals(Triple(255, 0, 0), xtermRgb(196))
        assertEquals(Triple(255, 255, 255), xtermRgb(231))
        assertEquals(Triple(8, 8, 8), xtermRgb(232))
        assertEquals(Triple(238, 238, 238), xtermRgb(255))
    }
}
