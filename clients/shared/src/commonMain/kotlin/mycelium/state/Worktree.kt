package mycelium.state

import mycelium.api.Conversation
import mycelium.api.Project

fun worktreeFolder(projectPath: String, branch: String): String {
    val name = projectPath.trimEnd('/').substringAfterLast('/')

    return "$name-worktrees/${branch.replace('/', '-')}"
}

fun checkedOutBranch(project: Project, conversations: Collection<Conversation>): String? =
    conversations.filter { it.cwd.trimEnd('/') == project.path.trimEnd('/') && !it.branch.isNullOrBlank() }.maxByOrNull { it.updatedAt }?.branch

fun worktreeHint(project: Project, branch: String, conversations: Collection<Conversation>): String {
    val from = checkedOutBranch(project, conversations)?.let { ", from $it" }.orEmpty()
    val named = branch.trim()

    return if (named.isEmpty()) "Names the branch and its folder$from" else "Folder ${worktreeFolder(project.path, named)}$from"
}
