package mycelium.state

import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.api.Task

enum class Lane(val title: String) {
    NeedsInput("Needs input"),
    ToDo("To do"),
    Running("Running"),
    ToReview("To review"),
    Done("Done"),
}

data class BoardFilter(val text: String = "", val projectId: String? = null, val nodeId: String? = null, val agent: String? = null)

data class BoardCard(
    val key: String,
    val title: String,
    val agent: String,
    val model: String?,
    val projectId: String?,
    val projectName: String?,
    val branch: String?,
    val nodeId: String,
    val conversation: Conversation?,
    val task: Task?,
) {
    val details: String get() = listOfNotNull(agentName(agent), model, projectName, branch).joinToString(" · ")
}

private const val DoneCardLimit = 50

fun laneOf(status: ConversationStatus): Lane = when (status) {
    ConversationStatus.Input -> Lane.NeedsInput
    ConversationStatus.Working -> Lane.Running
    ConversationStatus.Review, ConversationStatus.Failed -> Lane.ToReview
    ConversationStatus.Done -> Lane.Done
}

fun boardCards(state: AppState, filter: BoardFilter = BoardFilter()): Map<Lane, List<BoardCard>> {
    val fromConversations = state.conversations.values
        .filter { it.archivedAt == null }
        .sortedByDescending { it.updatedAt }
        .map { conversation ->
            val worktree = state.worktrees.values.firstOrNull { it.path == conversation.cwd }
            val project = worktree?.let { state.projects[it.projectId] } ?: state.projects.values.firstOrNull { it.path == conversation.cwd }

            laneOf(conversation.status) to BoardCard(
                key = conversation.id,
                title = conversation.title,
                agent = conversation.agent,
                model = conversation.model,
                projectId = project?.id,
                projectName = project?.name,
                branch = worktree?.branch,
                nodeId = conversation.nodeId,
                conversation = conversation,
                task = null,
            )
        }
    val fromTasks = state.tasks.values.sortedBy { it.createdAt }.map { task ->
        val project = task.projectId?.let { state.projects[it] }

        Lane.ToDo to BoardCard(task.id, task.title, task.agent, null, project?.id, project?.name, task.branch, task.nodeId, null, task)
    }
    val matching = (fromConversations + fromTasks).filter { (_, card) -> matches(card, filter) }
    val lanes = matching.groupBy({ it.first }, { it.second })

    return Lane.entries.associateWith { lane -> lanes[lane].orEmpty().let { if (lane == Lane.Done) it.take(DoneCardLimit) else it } }
}

private fun matches(card: BoardCard, filter: BoardFilter): Boolean {
    val text = filter.text.trim()
    val searchable = listOfNotNull(card.title, card.projectName, card.branch).joinToString("\n")

    return (text.isEmpty() || searchable.contains(text, ignoreCase = true)) &&
        (filter.projectId == null || card.projectId == filter.projectId) &&
        (filter.nodeId == null || card.nodeId == filter.nodeId) &&
        (filter.agent == null || card.agent == filter.agent)
}

fun collapsedLanes(saved: String?): Set<Lane> =
    if (saved == null) setOf(Lane.Done) else saved.split(',').mapNotNull { name -> Lane.entries.firstOrNull { it.name == name } }.toSet()

fun savedLanes(collapsed: Set<Lane>): String = collapsed.joinToString(",") { it.name }

fun pendingPermission(events: List<mycelium.api.Event>): ThreadItem.Permission? =
    buildThread(events).filterIsInstance<ThreadItem.Permission>().lastOrNull { !it.resolved }
