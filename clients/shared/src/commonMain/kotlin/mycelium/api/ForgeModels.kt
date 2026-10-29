package mycelium.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
data class Forge(
    val id: String,
    val kind: String,
    @SerialName("base_url") val baseUrl: String,
    val account: String,
    @SerialName("token_hint") val tokenHint: String,
    val organisations: List<String> = emptyList(),
    @SerialName("created_at") val createdAt: String,
    @SerialName("mcp_server_id") val mcpServerId: String? = null,
    @SerialName("mcp_error") val mcpError: String? = null,
)

@Serializable
data class ConnectForgeRequest(val kind: String, @SerialName("base_url") val baseUrl: String? = null, val token: String, @SerialName("add_mcp") val addMcp: Boolean? = null)

@Serializable
data class Issue(
    @SerialName("forge_id") val forgeId: String,
    val repository: String,
    val number: Long,
    val title: String,
    val url: String,
    val body: String? = null,
    @SerialName("pull_requests") val pullRequests: List<PullRequest>? = null,
    val author: ForgePerson? = null,
    val assignees: List<ForgePerson> = emptyList(),
    @SerialName("updated_at") val updatedAt: String? = null,
)

@Serializable
data class ForgePerson(val name: String, @SerialName("avatar_url") val avatarUrl: String? = null)

@Serializable
enum class PullRequestState {
    @SerialName("open") Open,
    @SerialName("merged") Merged,
    @SerialName("closed") Closed,
}

@Serializable
data class PullRequest(val number: Long, val label: String, val url: String, val state: PullRequestState)

@Serializable
data class PullRequestItem(
    @SerialName("forge_id") val forgeId: String,
    val repository: String,
    val number: Long,
    val label: String,
    val title: String,
    val url: String,
    val body: String = "",
    val state: PullRequestState,
    val draft: Boolean = false,
    @SerialName("source_branch") val sourceBranch: String = "",
    @SerialName("target_branch") val targetBranch: String = "",
    val author: ForgePerson? = null,
    val assignees: List<ForgePerson> = emptyList(),
    @SerialName("updated_at") val updatedAt: String? = null,
)

@Serializable
data class PullRequests(@SerialName("pull_requests") val pullRequests: List<PullRequestItem> = emptyList(), val failures: List<RepositoryFailure> = emptyList())

@Serializable
data class PullRequestComment(
    val author: ForgePerson? = null,
    val body: String,
    @SerialName("created_at") val createdAt: String? = null,
    val path: String? = null,
    val resolved: Boolean? = null,
)

@Serializable
data class PullRequestDetail(@SerialName("pull_request") val pullRequest: PullRequestItem, val timeline: List<PullRequestComment> = emptyList())

@Serializable
data class PullRequestDiff(val diff: String)

@Serializable
data class Issues(val issues: List<Issue> = emptyList(), val failures: List<RepositoryFailure> = emptyList())

@Serializable
data class StartWorkspaceRequest(val agent: String)

@Serializable
data class Repository(
    @SerialName("forge_id") val forgeId: String,
    val owner: String,
    val name: String,
    @SerialName("full_name") val fullName: String,
    val description: String? = null,
    val private: Boolean = false,
    @SerialName("default_branch") val defaultBranch: String? = null,
    @SerialName("updated_at") val updatedAt: String? = null,
)

@Serializable
data class RepositoryOrganisation(@SerialName("forge_id") val forgeId: String, val name: String)

@Serializable
data class RepositoryFailure(@SerialName("forge_id") val forgeId: String, val message: String)

@Serializable
data class Repositories(
    val repositories: List<Repository> = emptyList(),
    val organisations: List<RepositoryOrganisation> = emptyList(),
    val failures: List<RepositoryFailure> = emptyList(),
)
