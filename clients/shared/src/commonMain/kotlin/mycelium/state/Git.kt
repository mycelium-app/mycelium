package mycelium.state

import mycelium.api.GitFile

sealed interface ChangeRow {
    data class Heading(val name: String) : ChangeRow

    data class Folder(val path: String) : ChangeRow

    data class Change(val file: GitFile, val name: String, val indented: Boolean) : ChangeRow
}

fun changeRows(files: List<GitFile>): List<ChangeRow> {
    val rows = mutableListOf<ChangeRow>()

    for ((heading, group) in listOf("Tracked" to files.filter { !it.untracked }, "Untracked" to files.filter { it.untracked })) {
        if (group.isEmpty()) {
            continue
        }

        rows += ChangeRow.Heading(heading)

        val byFolder = group.groupBy { it.path.substringBeforeLast('/', "") }

        for (folder in byFolder.keys.sortedWith(compareBy({ it.isNotEmpty() }, { it }))) {
            if (folder.isNotEmpty()) {
                rows += ChangeRow.Folder(folder)
            }

            for (file in byFolder.getValue(folder).sortedBy { it.path }) {
                rows += ChangeRow.Change(file, file.path.substringAfterLast('/'), indented = folder.isNotEmpty())
            }
        }
    }

    return rows
}

fun firstChange(files: List<GitFile>): GitFile? = files.firstOrNull { !it.untracked } ?: files.firstOrNull()

fun diffIsStaged(file: GitFile): Boolean = file.staged && !file.unstaged

fun fullyStaged(file: GitFile): Boolean = file.staged && !file.unstaged && !file.untracked

fun commitProblem(message: String, files: List<GitFile>): String? = when {
    message.isBlank() -> "Enter a commit message."
    files.none { it.staged } -> "Stage a change before you commit."
    else -> null
}

fun parseUnifiedDiff(path: String, text: String): FileDiff {
    val lines = mutableListOf<DiffLine>()
    var oldNumber = 0
    var newNumber = 0
    var inHunk = false

    for (raw in text.lines()) {
        val header = HunkHeader.matchEntire(raw)

        when {
            header != null -> {
                oldNumber = header.groupValues[1].toInt()
                newNumber = header.groupValues[2].toInt()
                inHunk = true
                lines += DiffLine(DiffLine.Kind.Context, null, null, raw)
            }
            !inHunk || raw.startsWith("\\") -> Unit
            raw.startsWith("+") -> lines += DiffLine(DiffLine.Kind.Added, null, newNumber++, raw.drop(1))
            raw.startsWith("-") -> lines += DiffLine(DiffLine.Kind.Removed, oldNumber++, null, raw.drop(1))
            raw.startsWith(" ") -> lines += DiffLine(DiffLine.Kind.Context, oldNumber++, newNumber++, raw.drop(1))
        }
    }

    return FileDiff(path, lines)
}

private val HunkHeader = Regex("""@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@.*""")
