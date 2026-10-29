package mycelium.state

private val LineSuffix = Regex(":\\d+(:\\d+)?$")

fun pathWithoutLine(text: String): String = text.replace(LineSuffix, "")

fun isFilePath(text: String): Boolean {
    val path = pathWithoutLine(text)

    if (path.length < 2 || path.any { it.isWhitespace() } || "://" in path) {
        return false
    }

    val rooted = path.startsWith("/") || path.startsWith("~/") || path.startsWith("./") || path.startsWith("../")
    val tail = path.trimEnd('/').substringAfterLast('/')
    val named = '/' in path && '.' in tail && !tail.startsWith('.') && !tail.endsWith('.')

    return rooted || named
}

fun parentFolder(path: String): String = pathWithoutLine(path).trimEnd('/').substringBeforeLast('/', "/").ifEmpty { "/" }
