package mycelium.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
data class GitFile(
    val path: String,
    val staged: Boolean,
    val unstaged: Boolean,
    val untracked: Boolean,
    val additions: Int,
    val deletions: Int,
)

@Serializable
data class GitStatus(
    val branch: String,
    val upstream: String? = null,
    val ahead: Int,
    val behind: Int,
    val files: List<GitFile>,
    @SerialName("prepared_message") val preparedMessage: String? = null,
)

@Serializable
data class GitUndone(val message: String = "")

@Serializable
data class GitCommit(val hash: String, val author: String, val date: String, val subject: String)

@Serializable
data class GitDiff(val diff: String, val truncated: Boolean)

@Serializable
data class ChangedFile(
    val path: String,
    val staged: Boolean,
    val unstaged: Boolean,
    val untracked: Boolean,
    val added: Int,
    val removed: Int,
    val rows: Int,
    val hunks: List<Int> = emptyList(),
    val truncated: Boolean = false,
    val fingerprint: String,
    @SerialName("staged_rows") val stagedRows: Int = 0,
)

@Serializable
data class ChangesLayout(val files: List<ChangedFile>)

@Serializable
data class RowRange(val path: String, val from: Int, val to: Int)

@Serializable
data class RowsRequest(val ranges: List<RowRange>)

@Serializable
data class WireRow(val k: String, val o: Int? = null, val n: Int? = null, val t: String)

@Serializable
data class RowsChunk(val path: String, val from: Int, val fingerprint: String, val rows: List<WireRow>)

@Serializable
data class ChangesRows(val ranges: List<RowsChunk>)

@Serializable
data class GitHunkRequest(val path: String, val row: Int, val fingerprint: String, val action: String)

@Serializable
data class GitFileRequest(val path: String, val content: String? = null, val expected: String? = null)

@Serializable
data class GitFileContent(val content: String)

@Serializable
data class GitPathsRequest(val paths: List<String>)

@Serializable
data class GitPullRequest(val rebase: Boolean = false)

@Serializable
data class GitPushRequest(val force: Boolean = false)

@Serializable
data class GitCommitRequest(
    val message: String,
    val amend: Boolean = false,
    val signoff: Boolean = false,
    @SerialName("no_verify") val noVerify: Boolean = false,
)
