package mycelium.state

private const val ContextLines = 2

sealed interface DiffEntry {
    data class Line(val line: DiffLine) : DiffEntry

    data class Skipped(val count: Int) : DiffEntry
}

fun proposalDiff(baseText: String, text: String): List<DiffEntry> {
    val lines = diffLines(baseText, text)
    val near = BooleanArray(lines.size)

    for ((index, line) in lines.withIndex()) {
        if (line.kind != DiffLine.Kind.Context) {
            for (around in (index - ContextLines)..(index + ContextLines)) {
                if (around in lines.indices) {
                    near[around] = true
                }
            }
        }
    }

    val entries = mutableListOf<DiffEntry>()
    var skipped = 0

    for ((index, line) in lines.withIndex()) {
        if (near[index]) {
            if (skipped > 0) {
                entries += DiffEntry.Skipped(skipped)
                skipped = 0
            }

            entries += DiffEntry.Line(line)
        } else {
            skipped++
        }
    }

    if (skipped > 0) {
        entries += DiffEntry.Skipped(skipped)
    }

    return entries
}

fun waitingProposals(proposals: Collection<mycelium.api.InstructionProposal>): List<mycelium.api.InstructionProposal> =
    proposals.filter { it.waiting }.sortedByDescending { it.createdAt }

fun decidedProposals(proposals: Collection<mycelium.api.InstructionProposal>): List<mycelium.api.InstructionProposal> =
    proposals.filter { !it.waiting }.sortedByDescending { it.decidedAt ?: it.createdAt }
