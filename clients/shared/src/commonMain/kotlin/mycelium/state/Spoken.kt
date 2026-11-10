package mycelium.state

const val SpokenLanguage = "mycelium-spoken"

private const val Fence = "```"

fun spokenVersion(reply: String): String {
    val spoken = parsedMarkdown(reply).filterIsInstance<MarkdownBlock.Code>().filter { it.language == SpokenLanguage }.map { it.text.trim() }.filter { it.isNotEmpty() }

    return if (spoken.isEmpty()) reply else spoken.joinToString("\n\n")
}

fun withoutSpoken(reply: String): String {
    val kept = mutableListOf<String>()
    var inSpoken = false

    for (line in reply.lines()) {
        val trimmed = line.trim()

        when {
            inSpoken -> inSpoken = trimmed != Fence
            trimmed.startsWith(Fence) && trimmed.removePrefix(Fence).trim() == SpokenLanguage -> inSpoken = true
            else -> kept += line
        }
    }

    return kept.joinToString("\n").trim()
}
