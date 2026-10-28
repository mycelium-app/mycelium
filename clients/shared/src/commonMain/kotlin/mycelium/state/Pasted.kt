package mycelium.state

const val PasteLineLimit = 12

const val PasteCharLimit = 800

private const val FirstPasteMarker = ''

private const val LastPasteMarker = ''

const val BadgeGap = "\u2009"

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

fun pastedSummary(message: String): String = parsePasted(message).joinToString("") { segment ->
    when (segment) {
        is PastedSegment.Plain -> segment.text
        is PastedSegment.Pasted -> "[${pastedLabel(segment.lines)}]"
    }
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
    text.map { char -> if (isPasteMarker(char)) pastes[char] ?: char.toString() else char.toString() }.joinToString("")

fun serialisedPastes(text: String, pastes: Map<Char, String>): String =
    text.map { char -> if (isPasteMarker(char)) pastes[char]?.let(::pastedBlock).orEmpty() else char.toString() }.joinToString("")

class PasteDisplay(private val original: String, pastes: Map<Char, String>, images: Map<Char, String> = emptyMap()) {
    private val labels: Map<Char, String> = original
        .filter { it in pastes || it in images }
        .associateWith { marker -> images[marker]?.let(::imageLabel) ?: pastedLabel(pastedLineCount(pastes.getValue(marker))) }

    fun labelOf(char: Char): String? = labels[char]

    fun toShown(offset: Int): Int = original.take(offset).sumOf(::shownWidth)

    fun toOriginal(offset: Int): Int {
        var shownAt = 0

        for ((index, char) in original.withIndex()) {
            val width = shownWidth(char)

            if (offset <= shownAt || (offset < shownAt + width && 2 * (offset - shownAt) < width)) {
                return index
            }

            shownAt += width
        }

        return original.length
    }

    fun badgeRanges(): Map<Char, IntRange> {
        val ranges = LinkedHashMap<Char, IntRange>()
        var shownAt = 0

        for (char in original) {
            val label = labels[char]

            if (label != null) {
                ranges[char] = (shownAt + BadgeGap.length) until (shownAt + BadgeGap.length + label.length)
            }

            shownAt += shownWidth(char)
        }

        return ranges
    }

    private fun shownWidth(char: Char): Int = labels[char]?.let { it.length + 2 * BadgeGap.length } ?: 1
}

data class MarkedText(val text: String, val cursor: Int, val images: Map<Char, Int>)

fun imageLabel(name: String): String = "Image · $name"

fun withImageMarkers(marked: MarkedText, ids: List<Int>, pasteMarkers: Set<Char>): MarkedText {
    var text = marked.text
    var cursor = marked.cursor.coerceIn(0, text.length)
    var images = marked.images

    for (id in ids) {
        val marker = freePasteMarker(pasteMarkers + images.keys + text.filter(::isPasteMarker).toSet()) ?: break

        text = text.substring(0, cursor) + marker + text.substring(cursor)
        cursor += 1
        images = images + (marker to id)
    }

    return MarkedText(text, cursor, images)
}

fun withoutImageMarkers(marked: MarkedText, ids: Set<Int>): MarkedText {
    val dropped = marked.images.filterValues { it in ids }.keys
    val cursorShift = marked.text.take(marked.cursor).count { it in dropped }

    return MarkedText(marked.text.filterNot { it in dropped }, marked.cursor - cursorShift, marked.images - dropped)
}

fun orphanedImageIds(text: String, images: Map<Char, Int>): List<Int> = images.filterKeys { it !in text }.values.toList()
