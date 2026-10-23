package mycelium.api

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
