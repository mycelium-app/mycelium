package mycelium.state

import mycelium.api.Walkthrough
import mycelium.api.WalkthroughFile
import mycelium.api.WalkthroughNote
import mycelium.api.WalkthroughStep

fun walkthroughDiff(file: WalkthroughFile): FileDiff {
    if (file.diff.lines().any { it.startsWith("@@") }) {
        return parseUnifiedDiff(file.path, file.diff)
    }

    val parsed = parseUnifiedDiff(file.path, "@@ -1 +1 @@\n${file.diff}")

    return parsed.copy(lines = parsed.lines.drop(1).map { it.copy(oldNumber = null, newNumber = null) })
}

fun stepPosition(index: Int, total: Int): String = "Step ${index + 1} of $total"

enum class WalkthroughScope(val label: String, val subject: String, val key: String) {
    Branch("This branch", "this branch against its base branch", "branch"),
    Uncommitted("Uncommitted changes", "the uncommitted changes in the working tree", "uncommitted"),
    LastCommit("Last commit", "the last commit", "last_commit"),
}

fun iterateWalkthroughPrompt(scope: WalkthroughScope, revising: Boolean): String =
    if (revising) {
        "Revise the current walkthrough of ${scope.subject} using the notes and the comment, with the walkthrough tool, passing scope \"${scope.key}\". Keep the steps the notes do not touch, group the steps into named sections in reading order, and mark exactly one step as the key change."
    } else {
        "Write a new walkthrough of ${scope.subject} with the walkthrough tool, passing scope \"${scope.key}\". Group the steps into named sections in reading order, and mark exactly one step as the key change."
    }

data class WalkthroughSection(val name: String?, val icon: String?, val steps: List<IndexedValue<WalkthroughStep>>)

fun walkthroughSections(steps: List<WalkthroughStep>): List<WalkthroughSection> =
    steps.withIndex()
        .groupBy { it.value.section }
        .entries
        .sortedBy { it.key != null }
        .map { (name, grouped) -> WalkthroughSection(name, grouped.firstNotNullOfOrNull { it.value.icon }, grouped) }

fun walkthroughLines(diff: FileDiff, quote: String): Pair<Int, Int>? {
    val quoted = quote.lines().map { it.trim() }.filter { it.isNotEmpty() }.toSet()
    val numbers = diff.lines.filter { line -> line.text.isNotBlank() && quoted.any { it.contains(line.text.trim()) || line.text.contains(it) } }.mapNotNull { it.newNumber ?: it.oldNumber }

    return if (numbers.isEmpty()) null else numbers.min() to numbers.max()
}

fun walkthroughNoteLabel(walkthrough: Walkthrough, note: WalkthroughNote): String {
    val title = walkthrough.steps.getOrNull(note.step)?.title.orEmpty()
    val lines = when {
        note.lineStart == null -> ""
        note.lineEnd == null || note.lineEnd == note.lineStart -> " line ${note.lineStart}"
        else -> " lines ${note.lineStart}-${note.lineEnd}"
    }
    val place = note.file?.let { ", $it$lines" }.orEmpty()
    val comment = note.comment.ifBlank { "(no comment)" }

    return "Step ${note.step + 1} '$title'$place: $comment"
}

fun walkthroughNotesBlock(walkthrough: Walkthrough, notes: List<WalkthroughNote>): String? {
    val unsent = notes.filter { !it.sent }

    if (unsent.isEmpty()) {
        return null
    }

    val lines = unsent.map { note ->
        val quote = note.quote.lines().filter { it.isNotBlank() }.joinToString("") { "\n  > $it" }

        "- ${walkthroughNoteLabel(walkthrough, note)}$quote"
    }

    return "Notes on the walkthrough:\n" + lines.joinToString("\n")
}
