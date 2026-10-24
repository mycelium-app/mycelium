package mycelium.state

const val PasteLineLimit = 12

const val PasteCharLimit = 800

private const val FirstPasteMarker = ''

private const val LastPasteMarker = ''

private val PastedBlock = Regex("""<pasted-text lines="(\d+)">\n([\s\S]*?)\n</pasted-text>""")

sealed class PastedSegment {
    data class Plain(val text: String) : PastedSegment()

    data class Pasted(val text: String) : PastedSegment() {
        val lines: Int get() = pastedLineCount(text)
    }
}

fun pastedLineCount(text: String): Int = text.count { it == '\n' } + 1

fun pastedLabel(lines: Int): String = "Pasted text · $lines ${if (lines == 1) "line" else "lines"}"

fun isLargePaste(text: String): Boolean = pastedLineCount(text) > PasteLineLimit || text.length > PasteCharLimit

fun isPasteMarker(char: Char): Boolean = char in FirstPasteMarker..LastPasteMarker

fun freePasteMarker(used: Set<Char>): Char? = (FirstPasteMarker..LastPasteMarker).firstOrNull { it !in used }

fun pastedBlock(text: String): String = "<pasted-text lines=\"${pastedLineCount(text)}\">\n$text\n</pasted-text>"

fun parsePasted(message: String): List<PastedSegment> {
    val segments = mutableListOf<PastedSegment>()
    var from = 0

    for (match in PastedBlock.findAll(message)) {
        val content = match.groupValues[2]

        if (match.groupValues[1] != pastedLineCount(content).toString()) {
            continue
        }

        if (match.range.first > from) {
            segments += PastedSegment.Plain(message.substring(from, match.range.first))
        }

        segments += PastedSegment.Pasted(content)
        from = match.range.last + 1
    }

    if (from < message.length) {
        segments += PastedSegment.Plain(message.substring(from))
    }

    return segments
}

fun pasteInsertion(before: String, after: String): IntRange? {
    val prefix = before.commonPrefixWith(after).length
    val suffix = before.commonSuffixWith(after).length.coerceAtMost(minOf(before.length, after.length) - prefix)
    val end = after.length - suffix

    if (end <= prefix || !isLargePaste(after.substring(prefix, end))) {
        return null
    }

    return prefix until end
}

fun expandedPastes(text: String, pastes: Map<Char, String>): String =
    text.map { char -> if (isPasteMarker(char)) pastes[char].orEmpty() else char.toString() }.joinToString("")

fun serialisedPastes(text: String, pastes: Map<Char, String>): String =
    text.map { char -> if (isPasteMarker(char)) pastes[char]?.let(::pastedBlock).orEmpty() else char.toString() }.joinToString("")
