package mycelium.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
data class GitRef(val name: String, val kind: String, val current: Boolean = false)

@Serializable
data class GraphCommit(val hash: String, val parents: List<String>, val author: String, val date: String, val refs: List<GitRef>, val subject: String)

@Serializable
data class GitBranch(val name: String, val author: String, val date: String, val subject: String)

@Serializable
data class GitBranches(val current: String, val local: List<GitBranch>, val remote: List<GitBranch>)

@Serializable
data class GitSwitchRequest(val branch: String, val create: Boolean = false, val track: String? = null)

@Serializable
data class GitStackPullRequest(val number: Long, val label: String, val title: String, val url: String, val state: String)

@Serializable
data class GitStackBranch(
    val name: String,
    val parent: String? = null,
    val depth: Int = 0,
    val current: Boolean = false,
    val main: Boolean = false,
    val ahead: Int = 0,
    val behind: Int = 0,
    @SerialName("has_upstream") val hasUpstream: Boolean = false,
    val pushed: Boolean = false,
    @SerialName("needs_sync") val needsSync: Boolean = false,
    val subject: String = "",
    @SerialName("pull_request") val pullRequest: GitStackPullRequest? = null,
)

@Serializable
data class GitStack(
    val current: String,
    val main: String? = null,
    val inferred: Boolean = false,
    @SerialName("git_town_available") val gitTownAvailable: Boolean = false,
    val branches: List<GitStackBranch> = emptyList(),
    @SerialName("pull_requests_available") val pullRequestsAvailable: Boolean = false,
    @SerialName("pull_requests_problem") val pullRequestsProblem: String? = null,
)
