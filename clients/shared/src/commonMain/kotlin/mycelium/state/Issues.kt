package mycelium.state

import mycelium.api.Issue
import mycelium.api.Project
import mycelium.api.PullRequest
import mycelium.api.PullRequestState
import mycelium.api.Worktree

data class RepositoryCount(val forgeId: String, val repository: String, val count: Int)

fun projectsOfIssue(projects: Collection<Project>, issue: Issue): List<Project> =
    projects.filter { it.forgeId == issue.forgeId && it.repository == issue.repository }

fun worktreesOfIssue(projects: Collection<Project>, worktrees: List<Worktree>, issue: Issue): List<Worktree> {
    val ids = projectsOfIssue(projects, issue).map { it.id }.toSet()

    return worktrees.filter { it.projectId in ids && it.issueNumber == issue.number }
}

fun issueCounts(issues: List<Issue>): List<RepositoryCount> =
    issues.groupBy { it.forgeId to it.repository }.map { (key, group) -> RepositoryCount(key.first, key.second, group.size) }.sortedBy { it.repository }

fun pullRequestText(request: PullRequest): String =
    "${request.label} ${when (request.state) {
        PullRequestState.Open -> "open"
        PullRequestState.Merged -> "merged"
        PullRequestState.Closed -> "closed"
    }}"
