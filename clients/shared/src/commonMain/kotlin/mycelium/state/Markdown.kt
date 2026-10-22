package mycelium.state

sealed interface MarkdownBlock {
    data class Paragraph(val text: String) : MarkdownBlock

    data class Heading(val level: Int, val text: String) : MarkdownBlock

    data class Code(val language: String, val text: String) : MarkdownBlock

    data class Bullets(val items: List<String>) : MarkdownBlock

    data class Numbered(val start: Int, val items: List<String>) : MarkdownBlock

    data class Quote(val text: String) : MarkdownBlock

    data object Rule : MarkdownBlock
}

private val HeadingLine = Regex("^(#{1,6})\\s+(.*)$")
private val BulletLine = Regex("^\\s*[-*+]\\s+(.*)$")
private val NumberedLine = Regex("^\\s*(\\d+)[.)]\\s+(.*)$")
private val RuleLine = Regex("^\\s*([-*_])(\\s*\\1){2,}\\s*$")

fun markdownBlocks(text: String): List<MarkdownBlock> {
    val blocks = mutableListOf<MarkdownBlock>()
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

            RuleLine.matches(line) -> {
                blocks += MarkdownBlock.Rule
                index++
            }

            HeadingLine.matches(trimmed) -> {
                val match = HeadingLine.find(trimmed)!!
                blocks += MarkdownBlock.Heading(match.groupValues[1].length, match.groupValues[2].trimEnd('#', ' '))
                index++
            }

            BulletLine.matches(line) -> {
                val items = mutableListOf<String>()

                while (index < lines.size && BulletLine.matches(lines[index])) {
                    items += BulletLine.find(lines[index])!!.groupValues[1]
                    index++
                }

                blocks += MarkdownBlock.Bullets(items)
            }

            NumberedLine.matches(line) -> {
                val start = NumberedLine.find(line)!!.groupValues[1].toInt()
                val items = mutableListOf<String>()

                while (index < lines.size && NumberedLine.matches(lines[index])) {
                    items += NumberedLine.find(lines[index])!!.groupValues[2]
                    index++
                }

                blocks += MarkdownBlock.Numbered(start, items)
            }

            trimmed.startsWith(">") -> {
                val quoted = mutableListOf<String>()

                while (index < lines.size && lines[index].trimStart().startsWith(">")) {
                    quoted += lines[index].trimStart().removePrefix(">").removePrefix(" ")
                    index++
                }

                blocks += MarkdownBlock.Quote(quoted.joinToString("\n"))
            }

            else -> {
                val paragraph = mutableListOf<String>()

                while (index < lines.size && lines[index].isNotBlank() && !startsBlock(lines[index])) {
                    paragraph += lines[index].trim()
                    index++
                }

                blocks += MarkdownBlock.Paragraph(paragraph.joinToString("\n"))
            }
        }
    }

    return blocks
}

private fun startsBlock(line: String): Boolean {
    val trimmed = line.trimStart()

    return trimmed.startsWith("```") || trimmed.startsWith(">") || HeadingLine.matches(trimmed) || BulletLine.matches(line) || NumberedLine.matches(line) || RuleLine.matches(line)
}
