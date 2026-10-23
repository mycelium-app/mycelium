package mycelium.state

import mycelium.api.GitBranch
import mycelium.api.GitBranches
import mycelium.api.GraphCommit

data class GraphEdge(val from: Int, val to: Int, val lane: Int)

data class GraphRow(
    val commit: GraphCommit,
    val column: Int,
    val lane: Int,
    val above: List<GraphEdge>,
    val below: List<GraphEdge>,
    val through: List<GraphEdge>,
    val width: Int,
) {
    val isMerge: Boolean get() = commit.parents.size > 1
}

private data class Track(val hash: String, val lane: Int)

fun graphRows(commits: List<GraphCommit>): List<GraphRow> {
    val tracks = mutableListOf<Track?>()
    var nextLane = 0
    val rows = mutableListOf<GraphRow>()

    for (commit in commits) {
        val matches = tracks.indices.filter { tracks[it]?.hash == commit.hash }
        val column = matches.firstOrNull() ?: freeColumn(tracks)
        val lane = matches.firstOrNull()?.let { tracks.at(it).lane } ?: nextLane++
        val above = matches.map { GraphEdge(it, column, tracks.at(it).lane) }
        val through = tracks.indices.filter { tracks[it] != null && it !in matches }.map { GraphEdge(it, it, tracks.at(it).lane) }
        val below = mutableListOf<GraphEdge>()

        matches.forEach { tracks[it] = null }

        commit.parents.forEachIndexed { index, parent ->
            val existing = tracks.indexOfFirst { it?.hash == parent }

            when {
                existing >= 0 -> below += GraphEdge(column, existing, tracks.at(existing).lane)
                index == 0 -> {
                    place(tracks, column, Track(parent, lane))
                    below += GraphEdge(column, column, lane)
                }
                else -> {
                    val slot = freeColumn(tracks)

                    place(tracks, slot, Track(parent, nextLane))
                    below += GraphEdge(column, slot, nextLane++)
                }
            }
        }

        while (tracks.lastOrNull() == null && tracks.isNotEmpty()) {
            tracks.removeAt(tracks.lastIndex)
        }

        val width = (above + through + below).maxOfOrNull { maxOf(it.from, it.to) + 1 }?.coerceAtLeast(column + 1) ?: (column + 1)

        rows += GraphRow(commit, column, lane, above, below, through, width)
    }

    return rows
}

fun headLane(rows: List<GraphRow>): Int? = rows.firstOrNull { row -> row.commit.refs.any { it.current } }?.lane

private fun freeColumn(tracks: List<Track?>): Int = tracks.indexOfFirst { it == null }.let { if (it >= 0) it else tracks.size }

private fun place(tracks: MutableList<Track?>, column: Int, track: Track) {
    while (tracks.size <= column) {
        tracks += null
    }

    tracks[column] = track
}

fun splitCommitDiff(text: String): List<FileDiff> {
    val chunks = mutableListOf<MutableList<String>>()

    for (line in text.lines()) {
        if (line.startsWith("diff --git ")) {
            chunks += mutableListOf(line)
        } else {
            chunks.lastOrNull()?.add(line)
        }
    }

    return chunks.map { chunk ->
        val path = chunk.first().substringAfterLast(" b/")

        parseUnifiedDiff(path, chunk.joinToString("\n"))
    }
}

fun graphDescription(commit: GraphCommit, now: kotlin.time.Instant = kotlin.time.Clock.System.now()): String {
    val refs = commit.refs.joinToString(", ") { refDescription(it) }

    return "${commit.subject}, ${commit.author}, ${ago(commit.date, now)}" + if (refs.isEmpty()) "" else ", $refs"
}

fun refDescription(ref: mycelium.api.GitRef): String = when {
    ref.kind == "head" -> "detached HEAD"
    ref.kind == "tag" -> "tag ${ref.name}"
    ref.kind == "remote" -> "remote branch ${ref.name}"
    ref.current -> "current branch ${ref.name}"
    else -> "branch ${ref.name}"
}

sealed interface BranchEntry {
    data class Local(val branch: GitBranch, val current: Boolean) : BranchEntry

    data class Remote(val branch: GitBranch, val localName: String) : BranchEntry

    data class Create(val name: String) : BranchEntry
}

fun branchEntries(branches: GitBranches, search: String): List<BranchEntry> {
    val needle = search.trim()
    val local = branches.local.filter { it.name.contains(needle, ignoreCase = true) }
    val localNames = branches.local.map { it.name }.toSet()
    val remote = branches.remote
        .filter { it.name.contains(needle, ignoreCase = true) }
        .map { BranchEntry.Remote(it, it.name.substringAfter('/')) }
        .filter { it.localName !in localNames }
    val named = needle.isNotEmpty() && needle !in localNames && branches.remote.none { it.name == needle }

    return local.map { BranchEntry.Local(it, it.name == branches.current) } + remote + if (named) listOf(BranchEntry.Create(needle)) else emptyList()
}

private fun List<Track?>.at(column: Int): Track = requireNotNull(this[column])
