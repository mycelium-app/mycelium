package mycelium.state

import mycelium.api.WalkthroughFile

fun walkthroughDiff(file: WalkthroughFile): FileDiff {
    if (file.diff.lines().any { it.startsWith("@@") }) {
        return parseUnifiedDiff(file.path, file.diff)
    }

    val parsed = parseUnifiedDiff(file.path, "@@ -1 +1 @@\n${file.diff}")

    return parsed.copy(lines = parsed.lines.drop(1).map { it.copy(oldNumber = null, newNumber = null) })
}

fun stepPosition(index: Int, total: Int): String = "Step ${index + 1} of $total"
