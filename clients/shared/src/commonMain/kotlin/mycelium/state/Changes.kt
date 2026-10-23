package mycelium.state

import mycelium.api.ChangedFile
import mycelium.api.GitFile
import mycelium.api.RowRange
import mycelium.api.WireRow

const val LargeDiffRows = 2_000
const val RowChunk = 200

data class DiffSection(
    val path: String,
    val tag: String?,
    val added: Int,
    val removed: Int,
    val rowCount: Int,
    val hunks: List<Int>,
    val truncated: Boolean = false,
    val canStage: Boolean = false,
    val canUnstage: Boolean = false,
    val fingerprint: String = "",
)

class DiffLayout(val sections: List<DiffSection>, collapsed: Set<String>, expanded: Set<String>, limit: Int = LargeDiffRows) {
    val shown = IntArray(sections.size)
    val hiddenRows = IntArray(sections.size)
    val empty = BooleanArray(sections.size)
    val cut = BooleanArray(sections.size)
    val starts = IntArray(sections.size)
    val size: Int

    init {
        var next = 0

        for ((index, section) in sections.withIndex()) {
            val open = section.path !in collapsed

            shown[index] = if (!open) 0 else if (section.path in expanded) section.rowCount else minOf(section.rowCount, limit)
            hiddenRows[index] = if (open) section.rowCount - shown[index] else 0
            empty[index] = open && section.rowCount == 0
            cut[index] = open && section.truncated
            starts[index] = next
            next += itemCount(index)
        }

        size = next
    }

    fun itemCount(section: Int): Int =
        1 + shown[section] + (if (hiddenRows[section] > 0) 1 else 0) + (if (empty[section]) 1 else 0) + (if (cut[section]) 1 else 0)

    fun sectionAt(item: Int): Int {
        var low = 0
        var high = sections.lastIndex

        while (low < high) {
            val middle = (low + high + 1) / 2

            if (starts[middle] <= item) low = middle else high = middle - 1
        }

        return low
    }

    fun headerIndex(path: String): Int = sections.indexOfFirst { it.path == path }.let { if (it < 0) -1 else starts[it] }

    fun keyIndex(key: String): Int {
        if (key.startsWith("h:")) {
            return headerIndex(key.removePrefix("h:"))
        }

        val row = key.removePrefix("k:").substringBefore(':')
        val path = key.removePrefix("k:").substringAfter(':')
        val header = headerIndex(path)

        return if (header < 0) -1 else header + 1 + row.toInt()
    }

    fun windows(first: Int, last: Int, margin: Int): List<RowRange> {
        if (sections.isEmpty() || size == 0) return emptyList()

        val from = (first - margin).coerceAtLeast(0)
        val to = (last + margin).coerceAtMost(size - 1)
        val windows = mutableListOf<RowRange>()

        for (section in sectionAt(from)..sectionAt(to)) {
            val bodyStart = starts[section] + 1
            val low = (from - bodyStart).coerceAtLeast(0)
            val high = (to - bodyStart + 1).coerceAtMost(shown[section])

            if (high > low) {
                windows += RowRange(sections[section].path, low, high)
            }
        }

        return windows
    }

    fun stops(byFile: Boolean): List<Pair<Int, String>> {
        val stops = mutableListOf<Pair<Int, String>>()

        for ((index, section) in sections.withIndex()) {
            val open = shown[index] > 0
            val hunks = section.hunks.filter { it < shown[index] }

            if (byFile || !open || hunks.isEmpty()) {
                stops += starts[index] to "h:${section.path}"
            }

            if (!byFile) {
                for (hunk in hunks) {
                    stops += starts[index] + 1 + hunk to "k:$hunk:${section.path}"
                }
            }
        }

        return stops
    }
}

fun fileTag(file: GitFile): String = when {
    file.untracked -> "Untracked"
    file.staged && file.unstaged -> "Staged and unstaged"
    file.staged -> "Staged"
    else -> "Unstaged"
}

fun changeSections(layout: List<ChangedFile>): List<DiffSection> {
    val byPath = layout.associateBy { it.path }
    val files = layout.map { GitFile(it.path, it.staged, it.unstaged, it.untracked, it.added, it.removed) }
    val ordered = changeRows(files).filterIsInstance<ChangeRow.Change>().map { it.file }

    return ordered.map { file ->
        val changed = byPath.getValue(file.path)

        DiffSection(
            path = file.path,
            tag = fileTag(file),
            added = changed.added,
            removed = changed.removed,
            rowCount = changed.rows,
            hunks = changed.hunks,
            truncated = changed.truncated,
            canStage = file.unstaged || file.untracked,
            canUnstage = file.staged,
            fingerprint = changed.fingerprint,
        )
    }
}

fun commitSections(diffs: List<FileDiff>, truncated: Boolean): List<DiffSection> =
    diffs.mapIndexed { index, diff ->
        DiffSection(
            path = diff.path,
            tag = null,
            added = diff.added,
            removed = diff.removed,
            rowCount = diff.lines.size,
            hunks = diff.lines.indices.filter { isHunkHeader(diff.lines[it]) },
            truncated = truncated && index == diffs.lastIndex,
        )
    }

fun isHunkHeader(line: DiffLine): Boolean = line.kind == DiffLine.Kind.Context && line.oldNumber == null && line.newNumber == null

fun diffLine(row: WireRow): DiffLine = when (row.k) {
    "a" -> DiffLine(DiffLine.Kind.Added, null, row.n, row.t)
    "d" -> DiffLine(DiffLine.Kind.Removed, row.o, null, row.t)
    "c" -> DiffLine(DiffLine.Kind.Context, row.o, row.n, row.t)
    else -> DiffLine(DiffLine.Kind.Context, null, null, row.t)
}

fun focusStep(layout: DiffLayout, current: String?, step: Int, byFile: Boolean): String? {
    val stops = layout.stops(byFile)
    val position = if (current == null) -1 else layout.keyIndex(current)
    val target = if (step > 0) stops.firstOrNull { it.first > position } else stops.lastOrNull { position < 0 || it.first < position }

    return target?.second ?: current
}
