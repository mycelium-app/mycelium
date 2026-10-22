package mycelium.state

sealed interface TerminalColor {
    data object Default : TerminalColor

    data class Indexed(val index: Int) : TerminalColor

    data class Rgb(val red: Int, val green: Int, val blue: Int) : TerminalColor
}

data class CellStyle(
    val foreground: TerminalColor = TerminalColor.Default,
    val background: TerminalColor = TerminalColor.Default,
    val bold: Boolean = false,
    val dim: Boolean = false,
    val italic: Boolean = false,
    val underline: Boolean = false,
    val inverse: Boolean = false,
)

data class Cell(val codePoint: Int = ' '.code, val style: CellStyle = CellStyle())

private enum class Mode {
    Ground,
    Escape,
    Designate,
    Csi,
    Osc,
    OscEscape,
    String,
    StringEscape,
}

private const val Escape = 0x1B
private const val Bell = 0x07
private const val Replacement = 0xFFFD
private const val TabWidth = 8

fun codePointString(codePoint: Int): String =
    if (codePoint < 0x10000) {
        codePoint.toChar().toString()
    } else {
        val offset = codePoint - 0x10000

        charArrayOf((0xD800 + (offset shr 10)).toChar(), (0xDC00 + (offset and 0x3FF)).toChar()).concatToString()
    }

fun xtermRgb(index: Int): Triple<Int, Int, Int> {
    if (index in 16..231) {
        val level = { part: Int -> if (part == 0) 0 else 55 + part * 40 }
        val cube = index - 16

        return Triple(level(cube / 36), level(cube / 6 % 6), level(cube % 6))
    }

    val grey = 8 + (index - 232).coerceIn(0, 23) * 10

    return Triple(grey, grey, grey)
}

class Terminal(columns: Int, rows: Int) {
    var columns: Int = columns.coerceAtLeast(1)
        private set
    var rows: Int = rows.coerceAtLeast(1)
        private set
    var cursorRow = 0
        private set
    var cursorColumn = 0
        private set
    var cursorVisible = true
        private set
    var alternateScreen = false
        private set
    var applicationCursorKeys = false
        private set

    private var main = blankGrid(this.columns, this.rows)
    private var alternate = blankGrid(this.columns, this.rows)
    private var style = CellStyle()
    private var wrapPending = false
    private var autowrap = true
    private var scrollTop = 0
    private var scrollBottom = this.rows - 1
    private var saved = SavedCursor(0, 0, CellStyle(), false)
    private var mainSaved = SavedCursor(0, 0, CellStyle(), false)

    private var mode = Mode.Ground
    private var parameters = StringBuilder()
    private var ignoredSequence = false
    private var utf8Pending = 0
    private var utf8Value = 0

    private val grid: Array<Array<Cell>>
        get() = if (alternateScreen) alternate else main

    fun cell(row: Int, column: Int): Cell = grid[row][column]

    fun lineText(row: Int): String = grid[row].joinToString("") { codePointString(it.codePoint) }.trimEnd()

    fun screenText(): String = (0 until rows).joinToString("\n") { lineText(it) }

    fun feed(bytes: ByteArray) {
        for (byte in bytes) {
            accept(byte.toInt() and 0xFF)
        }
    }

    fun feed(text: String) = feed(text.encodeToByteArray())

    fun resize(columns: Int, rows: Int) {
        val newColumns = columns.coerceAtLeast(1)
        val newRows = rows.coerceAtLeast(1)

        if (newColumns == this.columns && newRows == this.rows) {
            return
        }

        main = resized(main, newColumns, newRows)
        alternate = resized(alternate, newColumns, newRows)
        this.columns = newColumns
        this.rows = newRows
        scrollTop = 0
        scrollBottom = newRows - 1
        cursorRow = cursorRow.coerceAtMost(newRows - 1)
        cursorColumn = cursorColumn.coerceAtMost(newColumns - 1)
        wrapPending = false
    }

    private fun accept(byte: Int) {
        when (mode) {
            Mode.Ground -> ground(byte)
            Mode.Escape -> escape(byte)
            Mode.Designate -> mode = Mode.Ground
            Mode.Csi -> csi(byte)
            Mode.Osc -> when (byte) {
                Bell -> mode = Mode.Ground
                Escape -> mode = Mode.OscEscape
            }
            Mode.OscEscape -> mode = if (byte == '\\'.code) Mode.Ground else Mode.Osc
            Mode.String -> if (byte == Escape) mode = Mode.StringEscape
            Mode.StringEscape -> mode = if (byte == '\\'.code) Mode.Ground else Mode.String
        }
    }

    private fun ground(byte: Int) {
        if (utf8Pending > 0) {
            if (byte and 0xC0 == 0x80) {
                utf8Value = (utf8Value shl 6) or (byte and 0x3F)
                utf8Pending--

                if (utf8Pending == 0) {
                    print(utf8Value)
                }

                return
            }

            utf8Pending = 0
            print(Replacement)
        }

        when {
            byte < 0x20 -> control(byte)
            byte < 0x7F -> print(byte)
            byte == 0x7F -> Unit
            byte and 0xE0 == 0xC0 -> startUtf8(1, byte and 0x1F)
            byte and 0xF0 == 0xE0 -> startUtf8(2, byte and 0x0F)
            byte and 0xF8 == 0xF0 -> startUtf8(3, byte and 0x07)
            else -> print(Replacement)
        }
    }

    private fun startUtf8(continuation: Int, value: Int) {
        utf8Pending = continuation
        utf8Value = value
    }

    private fun control(byte: Int) {
        when (byte) {
            Escape -> mode = Mode.Escape
            Bell -> Unit
            0x08 -> {
                cursorColumn = (cursorColumn - 1).coerceAtLeast(0)
                wrapPending = false
            }
            0x09 -> {
                cursorColumn = ((cursorColumn / TabWidth + 1) * TabWidth).coerceAtMost(columns - 1)
                wrapPending = false
            }
            0x0A, 0x0B, 0x0C -> lineFeed()
            0x0D -> {
                cursorColumn = 0
                wrapPending = false
            }
        }
    }

    private fun escape(byte: Int) {
        mode = Mode.Ground

        when (byte.toChar()) {
            '[' -> {
                mode = Mode.Csi
                parameters = StringBuilder()
                ignoredSequence = false
            }
            ']' -> mode = Mode.Osc
            'P', 'X', '^', '_' -> mode = Mode.String
            '(', ')', '*', '+', '#', '%' -> mode = Mode.Designate
            '7' -> saved = SavedCursor(cursorRow, cursorColumn, style, wrapPending)
            '8' -> restoreCursor(saved)
            'D' -> lineFeed()
            'E' -> {
                cursorColumn = 0
                lineFeed()
            }
            'M' -> reverseLineFeed()
            'c' -> reset()
            else -> if (byte == Escape) mode = Mode.Escape
        }
    }

    private fun csi(byte: Int) {
        when {
            byte == Escape -> mode = Mode.Escape
            byte == 0x18 || byte == 0x1A -> mode = Mode.Ground
            byte < 0x20 -> control(byte)
            byte in 0x20..0x2F -> ignoredSequence = true
            byte in 0x30..0x3F -> parameters.append(byte.toChar())
            byte in 0x40..0x7E -> {
                mode = Mode.Ground

                if (!ignoredSequence) {
                    dispatch(byte.toChar())
                }
            }
            else -> mode = Mode.Ground
        }
    }

    private fun dispatch(final: Char) {
        var text = parameters.toString()
        val marker = text.firstOrNull()?.takeIf { it in "?<=>" }

        if (marker != null) {
            text = text.drop(1)
        }

        val values = if (text.isEmpty()) emptyList() else text.split(';', ':').map { it.toIntOrNull() }

        if (marker == '?') {
            privateMode(final, values)
        } else if (marker == null) {
            standard(final, values)
        }
    }

    private fun standard(final: Char, values: List<Int?>) {
        val first = values.getOrNull(0)
        val count = (first ?: 1).coerceAtLeast(1)

        when (final) {
            'A' -> moveRow(-count)
            'B', 'e' -> moveRow(count)
            'C', 'a' -> moveColumn(count)
            'D' -> moveColumn(-count)
            'E' -> {
                moveRow(count)
                cursorColumn = 0
            }
            'F' -> {
                moveRow(-count)
                cursorColumn = 0
            }
            'G', '`' -> setCursor(cursorRow, count - 1)
            'H', 'f' -> setCursor(count - 1, ((values.getOrNull(1) ?: 1).coerceAtLeast(1)) - 1)
            'd' -> setCursor(count - 1, cursorColumn)
            'J' -> eraseDisplay(first ?: 0)
            'K' -> eraseLine(first ?: 0)
            'L' -> insertLines(count)
            'M' -> deleteLines(count)
            '@' -> insertCharacters(count)
            'P' -> deleteCharacters(count)
            'X' -> eraseCharacters(count)
            'S' -> repeat(count) { scrollUp() }
            'T' -> repeat(count) { scrollDown() }
            'r' -> setScrollRegion(first ?: 1, values.getOrNull(1) ?: rows)
            'm' -> selectGraphics(values)
            's' -> saved = SavedCursor(cursorRow, cursorColumn, style, wrapPending)
            'u' -> restoreCursor(saved)
        }
    }

    private fun privateMode(final: Char, values: List<Int?>) {
        if (final != 'h' && final != 'l') {
            return
        }

        val enable = final == 'h'

        for (value in values.filterNotNull()) {
            when (value) {
                1 -> applicationCursorKeys = enable
                7 -> autowrap = enable
                25 -> cursorVisible = enable
                47, 1047 -> switchScreen(enable, clear = false, save = false)
                1049 -> switchScreen(enable, clear = true, save = true)
            }
        }
    }

    private fun switchScreen(toAlternate: Boolean, clear: Boolean, save: Boolean) {
        if (toAlternate == alternateScreen) {
            return
        }

        if (toAlternate) {
            if (save) {
                mainSaved = SavedCursor(cursorRow, cursorColumn, style, wrapPending)
            }

            alternateScreen = true

            if (clear) {
                alternate = blankGrid(columns, rows)
            }
        } else {
            alternateScreen = false

            if (save) {
                restoreCursor(mainSaved)
            }
        }
    }

    private fun print(codePoint: Int) {
        if (wrapPending) {
            if (autowrap) {
                cursorColumn = 0
                lineFeed()
            }

            wrapPending = false
        }

        grid[cursorRow][cursorColumn] = Cell(codePoint, style)

        if (cursorColumn == columns - 1) {
            wrapPending = autowrap
        } else {
            cursorColumn++
        }
    }

    private fun lineFeed() {
        if (cursorRow == scrollBottom) {
            scrollUp()
        } else if (cursorRow < rows - 1) {
            cursorRow++
        }
    }

    private fun reverseLineFeed() {
        if (cursorRow == scrollTop) {
            scrollDown()
        } else if (cursorRow > 0) {
            cursorRow--
        }
    }

    private fun scrollUp() {
        val lines = grid

        for (row in scrollTop until scrollBottom) {
            lines[row] = lines[row + 1]
        }

        lines[scrollBottom] = blankRow()
    }

    private fun scrollDown() {
        val lines = grid

        for (row in scrollBottom downTo scrollTop + 1) {
            lines[row] = lines[row - 1]
        }

        lines[scrollTop] = blankRow()
    }

    private fun insertLines(count: Int) {
        if (cursorRow !in scrollTop..scrollBottom) {
            return
        }

        val lines = grid
        val moved = count.coerceAtMost(scrollBottom - cursorRow + 1)

        for (row in scrollBottom downTo cursorRow + moved) {
            lines[row] = lines[row - moved]
        }

        for (row in cursorRow until cursorRow + moved) {
            lines[row] = blankRow()
        }

        cursorColumn = 0
        wrapPending = false
    }

    private fun deleteLines(count: Int) {
        if (cursorRow !in scrollTop..scrollBottom) {
            return
        }

        val lines = grid
        val moved = count.coerceAtMost(scrollBottom - cursorRow + 1)

        for (row in cursorRow..scrollBottom - moved) {
            lines[row] = lines[row + moved]
        }

        for (row in scrollBottom - moved + 1..scrollBottom) {
            lines[row] = blankRow()
        }

        cursorColumn = 0
        wrapPending = false
    }

    private fun insertCharacters(count: Int) {
        val line = grid[cursorRow]
        val moved = count.coerceAtMost(columns - cursorColumn)

        for (column in columns - 1 downTo cursorColumn + moved) {
            line[column] = line[column - moved]
        }

        for (column in cursorColumn until cursorColumn + moved) {
            line[column] = blankCell()
        }

        wrapPending = false
    }

    private fun deleteCharacters(count: Int) {
        val line = grid[cursorRow]
        val moved = count.coerceAtMost(columns - cursorColumn)

        for (column in cursorColumn..columns - 1 - moved) {
            line[column] = line[column + moved]
        }

        for (column in columns - moved until columns) {
            line[column] = blankCell()
        }

        wrapPending = false
    }

    private fun eraseCharacters(count: Int) {
        val line = grid[cursorRow]

        for (column in cursorColumn until (cursorColumn + count).coerceAtMost(columns)) {
            line[column] = blankCell()
        }
    }

    private fun eraseDisplay(mode: Int) {
        when (mode) {
            0 -> {
                eraseLine(0)

                for (row in cursorRow + 1 until rows) {
                    grid[row] = blankRow()
                }
            }
            1 -> {
                eraseLine(1)

                for (row in 0 until cursorRow) {
                    grid[row] = blankRow()
                }
            }
            2 -> {
                for (row in 0 until rows) {
                    grid[row] = blankRow()
                }
            }
        }
    }

    private fun eraseLine(mode: Int) {
        val line = grid[cursorRow]
        val range = when (mode) {
            0 -> cursorColumn until columns
            1 -> 0..cursorColumn
            2 -> 0 until columns
            else -> return
        }

        for (column in range) {
            line[column] = blankCell()
        }
    }

    private fun moveRow(delta: Int) {
        val floor = if (delta < 0 && cursorRow >= scrollTop) scrollTop else 0
        val ceiling = if (delta > 0 && cursorRow <= scrollBottom) scrollBottom else rows - 1

        cursorRow = (cursorRow + delta).coerceIn(floor, ceiling)
        wrapPending = false
    }

    private fun moveColumn(delta: Int) {
        cursorColumn = (cursorColumn + delta).coerceIn(0, columns - 1)
        wrapPending = false
    }

    private fun setCursor(row: Int, column: Int) {
        cursorRow = row.coerceIn(0, rows - 1)
        cursorColumn = column.coerceIn(0, columns - 1)
        wrapPending = false
    }

    private fun restoreCursor(from: SavedCursor) {
        setCursor(from.row, from.column)
        style = from.style
        wrapPending = from.wrapPending
    }

    private fun setScrollRegion(top: Int, bottom: Int) {
        val newTop = (top - 1).coerceIn(0, rows - 1)
        val newBottom = (bottom - 1).coerceIn(0, rows - 1)

        if (newTop < newBottom) {
            scrollTop = newTop
            scrollBottom = newBottom
        } else {
            scrollTop = 0
            scrollBottom = rows - 1
        }

        setCursor(0, 0)
    }

    private fun selectGraphics(values: List<Int?>) {
        if (values.isEmpty()) {
            style = CellStyle()

            return
        }

        var position = 0

        while (position < values.size) {
            val code = values[position] ?: 0
            position++

            when (code) {
                0 -> style = CellStyle()
                1 -> style = style.copy(bold = true)
                2 -> style = style.copy(dim = true)
                3 -> style = style.copy(italic = true)
                4 -> style = style.copy(underline = true)
                7 -> style = style.copy(inverse = true)
                22 -> style = style.copy(bold = false, dim = false)
                23 -> style = style.copy(italic = false)
                24 -> style = style.copy(underline = false)
                27 -> style = style.copy(inverse = false)
                in 30..37 -> style = style.copy(foreground = TerminalColor.Indexed(code - 30))
                39 -> style = style.copy(foreground = TerminalColor.Default)
                in 40..47 -> style = style.copy(background = TerminalColor.Indexed(code - 40))
                49 -> style = style.copy(background = TerminalColor.Default)
                in 90..97 -> style = style.copy(foreground = TerminalColor.Indexed(code - 90 + 8))
                in 100..107 -> style = style.copy(background = TerminalColor.Indexed(code - 100 + 8))
                38, 48 -> {
                    val color = when (values.getOrNull(position)) {
                        5 -> values.getOrNull(position + 1)?.let { TerminalColor.Indexed(it.coerceIn(0, 255)) }.also { position += 2 }
                        2 -> {
                            val channels = (1..3).map { values.getOrNull(position + it) }
                            position += 4

                            if (channels.all { it != null }) TerminalColor.Rgb(channels[0]!!.coerceIn(0, 255), channels[1]!!.coerceIn(0, 255), channels[2]!!.coerceIn(0, 255)) else null
                        }
                        else -> null
                    }

                    if (color != null) {
                        style = if (code == 38) style.copy(foreground = color) else style.copy(background = color)
                    }
                }
            }
        }
    }

    private fun reset() {
        main = blankGrid(columns, rows)
        alternate = blankGrid(columns, rows)
        style = CellStyle()
        cursorRow = 0
        cursorColumn = 0
        cursorVisible = true
        alternateScreen = false
        applicationCursorKeys = false
        autowrap = true
        wrapPending = false
        scrollTop = 0
        scrollBottom = rows - 1
    }

    private fun blankCell(): Cell = Cell(' '.code, CellStyle(background = style.background))

    private fun blankRow(): Array<Cell> = Array(columns) { blankCell() }

    private class SavedCursor(val row: Int, val column: Int, val style: CellStyle, val wrapPending: Boolean)
}

private fun blankGrid(columns: Int, rows: Int): Array<Array<Cell>> = Array(rows) { Array(columns) { Cell() } }

private fun resized(grid: Array<Array<Cell>>, columns: Int, rows: Int): Array<Array<Cell>> =
    Array(rows) { row -> Array(columns) { column -> grid.getOrNull(row)?.getOrNull(column) ?: Cell() } }

enum class TerminalKey { Enter, Backspace, Tab, Escape, Up, Down, Right, Left, Home, End, PageUp, PageDown, Delete }

fun terminalKeyInput(key: TerminalKey, applicationCursorKeys: Boolean = false): String {
    val arrow = if (applicationCursorKeys) "\u001BO" else "\u001B["

    return when (key) {
        TerminalKey.Enter -> "\r"
        TerminalKey.Backspace -> "\u007F"
        TerminalKey.Tab -> "\t"
        TerminalKey.Escape -> "\u001B"
        TerminalKey.Up -> "${arrow}A"
        TerminalKey.Down -> "${arrow}B"
        TerminalKey.Right -> "${arrow}C"
        TerminalKey.Left -> "${arrow}D"
        TerminalKey.Home -> "${arrow}H"
        TerminalKey.End -> "${arrow}F"
        TerminalKey.PageUp -> "\u001B[5~"
        TerminalKey.PageDown -> "\u001B[6~"
        TerminalKey.Delete -> "\u001B[3~"
    }
}

fun terminalControlInput(letter: Char): String? =
    if (letter.lowercaseChar() in 'a'..'z') (letter.lowercaseChar().code - 'a'.code + 1).toChar().toString() else null
