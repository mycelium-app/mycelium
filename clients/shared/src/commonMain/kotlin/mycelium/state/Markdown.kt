package mycelium.state

sealed interface MarkdownBlock {
    data class Paragraph(val text: String) : MarkdownBlock

    data class Heading(val level: Int, val text: String) : MarkdownBlock

    data class Code(val language: String, val text: String) : MarkdownBlock

    data class Bullets(val items: List<MarkdownListItem>) : MarkdownBlock

    data class Numbered(val start: Int, val items: List<MarkdownListItem>) : MarkdownBlock

    data class Quote(val blocks: List<MarkdownBlock>) : MarkdownBlock

    data class Table(val header: List<String>, val rows: List<List<String>>, val aligns: List<ColumnAlign> = emptyList()) : MarkdownBlock

    data class Callout(val kind: CalloutKind, val blocks: List<MarkdownBlock>) : MarkdownBlock

    data class Footnotes(val items: List<Footnote>) : MarkdownBlock

    data object Rule : MarkdownBlock
}

data class MarkdownListItem(val text: String, val checked: Boolean? = null, val children: List<MarkdownBlock> = emptyList())

data class Footnote(val label: String, val text: String)

enum class ColumnAlign { Start, Center, End }

enum class CalloutKind(val label: String) { Note("Note"), Tip("Tip"), Important("Important"), Warning("Warning"), Caution("Caution") }

private val HeadingLine = Regex("^(#{1,6})\\s+(.*)$")
private val ListLine = Regex("^(\\s*)([-*+]|\\d+[.)])\\s+(.*)$")
private val CalloutLine = Regex("^\\[!(NOTE|TIP|IMPORTANT|WARNING|CAUTION)]\\s*$", RegexOption.IGNORE_CASE)
private val FootnoteLine = Regex("^\\[\\^([^\\]\\s]+)]:\\s*(.*)$")
private val TaskText = Regex("^\\[([ xX])](?:\\s+(.*))?$", RegexOption.DOT_MATCHES_ALL)
private val RuleLine = Regex("^\\s*([-*_])(\\s*\\1){2,}\\s*$")
private val TableDivider = Regex("^\\s*\\|?\\s*:?-+:?\\s*(\\|\\s*:?-+:?\\s*)*\\|?\\s*$")

private fun alignOf(cell: String): ColumnAlign = when {
    cell.startsWith(":") && cell.endsWith(":") -> ColumnAlign.Center
    cell.endsWith(":") -> ColumnAlign.End
    else -> ColumnAlign.Start
}

private fun indentOf(line: String): Int = line.takeWhile { it == ' ' || it == '\t' }.sumOf { if (it == '\t') 4 else 1 }

private fun cells(line: String): List<String> = line.trim().removePrefix("|").removeSuffix("|").split("|").map { it.trim() }

private fun startsTable(lines: List<String>, index: Int): Boolean =
    lines[index].contains('|') && index + 1 < lines.size && lines[index + 1].contains('-') && TableDivider.matches(lines[index + 1])

private const val ParsedMessageLimit = 512
private val parsedMessages = LinkedHashMap<String, List<MarkdownBlock>>()

fun parsedMarkdown(text: String): List<MarkdownBlock> {
    val blocks = parsedMessages.remove(text) ?: markdownBlocks(text)
    parsedMessages[text] = blocks

    if (parsedMessages.size > ParsedMessageLimit) {
        parsedMessages.remove(parsedMessages.keys.first())
    }

    return blocks
}

fun markdownBlocks(text: String): List<MarkdownBlock> {
    val blocks = mutableListOf<MarkdownBlock>()
    val notes = mutableListOf<Footnote>()
    val lines = text.replace("\r\n", "\n").split("\n")
    var index = 0

    while (index < lines.size) {
        val line = lines[index]
        val trimmed = line.trimStart()

        when {
            trimmed.startsWith("```") -> {
                val language = trimmed.removePrefix("```").trim()
                val body = mutableListOf<String>()
                index++

                while (index < lines.size && !lines[index].trimStart().startsWith("```")) {
                    body += lines[index]
                    index++
                }

                blocks += MarkdownBlock.Code(language, body.joinToString("\n"))
                index++
            }

            line.isBlank() -> index++

            FootnoteLine.matches(trimmed) -> {
                val match = FootnoteLine.find(trimmed)!!
                notes += Footnote(match.groupValues[1], match.groupValues[2])
                index++
            }

            RuleLine.matches(line) -> {
                blocks += MarkdownBlock.Rule
                index++
            }

            startsTable(lines, index) -> {
                val header = cells(line)
                val aligns = cells(lines[index + 1]).map { alignOf(it) }
                val rows = mutableListOf<List<String>>()
                index += 2

                while (index < lines.size && lines[index].contains('|') && lines[index].isNotBlank()) {
                    rows += cells(lines[index]).let { row -> List(header.size) { column -> row.getOrElse(column) { "" } } }
                    index++
                }

                blocks += MarkdownBlock.Table(header, rows, List(header.size) { aligns.getOrElse(it) { ColumnAlign.Start } })
            }

            HeadingLine.matches(trimmed) -> {
                val match = HeadingLine.find(trimmed)!!
                blocks += MarkdownBlock.Heading(match.groupValues[1].length, match.groupValues[2].trimEnd('#', ' '))
                index++
            }

            ListLine.matches(line) -> {
                val base = indentOf(line)
                val ordered = ListLine.find(line)!!.groupValues[2].last().let { it == '.' || it == ')' }
                val starts = mutableListOf<Int>()
                val region = mutableListOf<String>()
                val start = if (ordered) ListLine.find(line)!!.groupValues[2].dropLast(1).toInt() else 1

                fun siblingOf(candidate: String): Boolean {
                    val match = ListLine.find(candidate) ?: return false

                    return indentOf(candidate) == base && match.groupValues[2].last().let { it == '.' || it == ')' } == ordered
                }

                while (index < lines.size) {
                    val current = lines[index]

                    if (current.isBlank()) {
                        val next = (index + 1 until lines.size).firstOrNull { lines[it].isNotBlank() }?.let { lines[it] }

                        if (next == null || (indentOf(next) <= base && !siblingOf(next))) {
                            break
                        }
                    } else if (siblingOf(current)) {
                        starts += region.size
                    } else if (indentOf(current) <= base) {
                        break
                    }

                    region += current
                    index++
                }

                val items = starts.mapIndexed { position, from ->
                    val to = starts.getOrElse(position + 1) { region.size }
                    val first = ListLine.find(region[from])!!.groupValues[3]
                    val rest = region.subList(from + 1, to)
                    val margin = rest.filter { it.isNotBlank() }.minOfOrNull { indentOf(it) } ?: 0
                    val nested = markdownBlocks(rest.joinToString("\n") { it.drop(minOf(margin, it.length - it.trimStart().length)) })
                    val continues = rest.firstOrNull()?.isNotBlank() == true && nested.firstOrNull() is MarkdownBlock.Paragraph
                    val body = if (continues) first + "\n" + (nested.first() as MarkdownBlock.Paragraph).text else first
                    val children = if (continues) nested.drop(1) else nested
                    val task = TaskText.find(body)

                    MarkdownListItem(task?.groupValues?.get(2) ?: body, task?.let { it.groupValues[1] != " " }, children)
                }

                blocks += if (ordered) MarkdownBlock.Numbered(start, items) else MarkdownBlock.Bullets(items)
            }

            trimmed.startsWith(">") -> {
                val quoted = mutableListOf<String>()

                while (index < lines.size && lines[index].trimStart().startsWith(">")) {
                    quoted += lines[index].trimStart().removePrefix(">").removePrefix(" ")
                    index++
                }

                val kind = CalloutLine.find(quoted.first())?.let { match -> CalloutKind.entries.first { it.name.equals(match.groupValues[1], ignoreCase = true) } }

                blocks += if (kind != null) {
                    MarkdownBlock.Callout(kind, markdownBlocks(quoted.drop(1).joinToString("\n")))
                } else {
                    MarkdownBlock.Quote(markdownBlocks(quoted.joinToString("\n")))
                }
            }

            else -> {
                val paragraph = mutableListOf<String>()

                while (index < lines.size && lines[index].isNotBlank() && !startsBlock(lines[index]) && !startsTable(lines, index)) {
                    paragraph += lines[index].trim()
                    index++
                }

                blocks += MarkdownBlock.Paragraph(paragraph.joinToString("\n"))
            }
        }
    }

    if (notes.isNotEmpty()) {
        blocks += MarkdownBlock.Footnotes(notes)
    }

    return blocks
}

private fun startsBlock(line: String): Boolean {
    val trimmed = line.trimStart()

    return trimmed.startsWith("```") || trimmed.startsWith(">") || FootnoteLine.matches(trimmed) || HeadingLine.matches(trimmed) || ListLine.matches(line) || RuleLine.matches(line)
}
