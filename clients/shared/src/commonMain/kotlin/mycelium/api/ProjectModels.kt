package mycelium.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
data class DirectoryEntry(val name: String, val path: String, val repository: Boolean = false)

@Serializable
data class Directories(val root: String, val path: String, val parent: String? = null, val directories: List<DirectoryEntry> = emptyList(), val truncated: Boolean = false)

@Serializable
data class Project(
    val id: String,
    @SerialName("node_id") val nodeId: String,
    val name: String,
    val path: String,
    @SerialName("forge_id") val forgeId: String? = null,
    val repository: String? = null,
    @SerialName("created_at") val createdAt: String,
    @SerialName("group_id") val groupId: String? = null,
    val icon: String? = null,
    @SerialName("avatar_sha256") val avatarSha256: String? = null,
)

@Serializable
data class PatchProjectRequest(
    @SerialName("group_id") val groupId: String? = null,
    val icon: String? = null,
    @SerialName("clear_icon") val clearIcon: Boolean = false,
)

@Serializable
data class Group(
    val id: String,
    val name: String,
    val folders: List<GroupFolder> = emptyList(),
    @SerialName("created_at") val createdAt: String,
    val hours: Hours = AlwaysOpenHours,
    @SerialName("open_now") val openNow: Boolean = true,
    @SerialName("closes_at") val closesAt: String? = null,
    @SerialName("opens_at") val opensAt: String? = null,
    @SerialName("opened_until") val openedUntil: String? = null,
    val icon: String? = null,
)

@Serializable
data class GroupFolder(val id: String, @SerialName("node_id") val nodeId: String? = null, val prefix: String)

@Serializable
data class GroupFolderRequest(val prefix: String, @SerialName("node_id") val nodeId: String? = null)

@Serializable
data class GroupRequest(val name: String, val icon: String? = null)

@Serializable
data class CreateProjectRequest(
    @SerialName("node_id") val nodeId: String,
    @SerialName("forge_id") val forgeId: String? = null,
    val repository: String? = null,
    val path: String? = null,
    @SerialName("new_repository") val newRepository: Boolean = false,
)

@Serializable
data class Worktree(
    val id: String,
    @SerialName("project_id") val projectId: String,
    val branch: String,
    val path: String,
    @SerialName("issue_number") val issueNumber: Long? = null,
    @SerialName("created_at") val createdAt: String,
)
