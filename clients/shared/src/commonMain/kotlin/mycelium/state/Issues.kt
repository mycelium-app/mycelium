package mycelium.state

import mycelium.api.Issue
import mycelium.api.PullRequest
import mycelium.api.PullRequestComment
import mycelium.api.PullRequestItem
import mycelium.api.PullRequestState

fun pullRequestText(request: PullRequest): String =
    "${request.label} ${when (request.state) {
        PullRequestState.Open -> "open"
        PullRequestState.Merged -> "merged"
        PullRequestState.Closed -> "closed"
    }}"

data class ForgeReference(val label: String, val title: String, val url: String)

private const val ReferenceLimit = 8

fun forgeReferences(issues: List<Issue>, pullRequests: List<PullRequestItem>, query: String): List<ForgeReference> {
    val needle = query.trim().removePrefix("#").lowercase()
    val all = issues.map { it.updatedAt to ForgeReference("#${it.number}", it.title, it.url) } +
        pullRequests.map { it.updatedAt to ForgeReference(it.label, it.title, it.url) }

    return all
        .filter { (_, reference) -> needle.isEmpty() || reference.title.lowercase().contains(needle) || reference.label.lowercase().contains(needle) }
        .sortedByDescending { (updated, _) -> updated.orEmpty() }
        .map { (_, reference) -> reference }
        .take(ReferenceLimit)
}

fun conversationPullRequestText(label: String?, state: String?): String? = label?.let { listOfNotNull(it, state).joinToString(" ") }

fun unresolvedOnly(timeline: List<PullRequestComment>): List<PullRequestComment> = timeline.filter { it.resolved != true }

fun pullRequestFileDiffs(diff: String): List<FileDiff> =
    diff.split(Regex("(?m)^diff --git ")).filter { it.isNotBlank() }.map { chunk ->
        val path = Regex("(?m)^\\+\\+\\+ b/(.*)$").find(chunk)?.groupValues?.get(1) ?: Regex("(?m)^--- a/(.*)$").find(chunk)?.groupValues?.get(1) ?: chunk.lineSequence().first().substringAfter(" b/")

        parseUnifiedDiff(path, chunk)
    }

fun initials(name: String): String = name.trim().take(2).uppercase()
