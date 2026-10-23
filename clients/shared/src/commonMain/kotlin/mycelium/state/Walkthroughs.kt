package mycelium.state

import mycelium.api.WalkthroughFile
import mycelium.api.WalkthroughStep

fun walkthroughDiff(file: WalkthroughFile): FileDiff {
    if (file.diff.lines().any { it.startsWith("@@") }) {
        return parseUnifiedDiff(file.path, file.diff)
    }

    val parsed = parseUnifiedDiff(file.path, "@@ -1 +1 @@\n${file.diff}")

    return parsed.copy(lines = parsed.lines.drop(1).map { it.copy(oldNumber = null, newNumber = null) })
}

fun stepPosition(index: Int, total: Int): String = "Step ${index + 1} of $total"

enum class WalkthroughScope(val label: String, val subject: String) {
    Branch("This branch", "this branch against its base branch"),
    Uncommitted("Uncommitted changes", "the uncommitted changes in the working tree"),
    LastCommit("Last commit", "the last commit"),
}

fun regenerateWalkthroughPrompt(scope: WalkthroughScope): String =
    "Write a new walkthrough of ${scope.subject} with the walkthrough tool. Group the steps into named sections in reading order, and mark exactly one step as the key change."

data class WalkthroughSection(val name: String?, val icon: String?, val steps: List<IndexedValue<WalkthroughStep>>)

fun walkthroughSections(steps: List<WalkthroughStep>): List<WalkthroughSection> =
    steps.withIndex()
        .groupBy { it.value.section }
        .entries
        .sortedBy { it.key != null }
        .map { (name, grouped) -> WalkthroughSection(name, grouped.firstNotNullOfOrNull { it.value.icon }, grouped) }
