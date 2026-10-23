package mycelium.state

import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.api.Task
import mycelium.api.TaskStatuses

enum class Lane(val title: String) {
    Questions("Questions"),
    Proposed("Proposed"),
    Open("Open"),
    Claimed("Claimed"),
    Done("Done"),
}

data class BoardFilter(val text: String = "", val projectId: String? = null, val nodeId: String? = null, val agent: String? = null, val showDeclined: Boolean = false)

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
    val details: String
        get() = when (task?.status) {
            TaskStatuses.Question -> "From ${task.sourceTitle ?: "a conversation that is gone"}"
            TaskStatuses.Proposed -> listOfNotNull(projectName, task.sourceTitle?.let { "from $it" }).joinToString(" · ")
            else -> listOfNotNull(agentName(agent), model, projectName, branch).joinToString(" · ")
        }
}

private const val DoneCardLimit = 50

fun laneOf(status: ConversationStatus): Lane = when (status) {
    ConversationStatus.Done -> Lane.Done
    else -> Lane.Claimed
}

fun laneOf(task: Task, showDeclined: Boolean): Lane? = when (task.status) {
    TaskStatuses.Question -> Lane.Questions
    TaskStatuses.Proposed -> Lane.Proposed
    TaskStatuses.Open -> Lane.Open
    TaskStatuses.Claimed -> Lane.Claimed
    TaskStatuses.Declined -> Lane.Done.takeIf { showDeclined }
    else -> Lane.Done
}

fun boardCards(state: AppState, filter: BoardFilter = BoardFilter()): Map<Lane, List<BoardCard>> {
    val claimedBy = state.tasks.values.mapNotNull { it.conversationId }.toSet()
    val fromConversations = state.conversations.values
        .filter { it.archivedAt == null && it.id !in claimedBy }
        .sortedByDescending { it.updatedAt }
        .map { conversation ->
            val worktree = state.worktrees.values.firstOrNull { it.path == conversation.cwd }
            val project = worktree?.let { state.projects[it.projectId] } ?: state.projects.values.firstOrNull { it.path == conversation.cwd }

            laneOf(conversation.status) to BoardCard(conversation.id, conversation.title, conversation.agent, conversation.model, project?.id, project?.name, worktree?.branch, conversation.nodeId, conversation, null)
        }
    val fromTasks = state.tasks.values.sortedBy { it.createdAt }.mapNotNull { task ->
        val lane = laneOf(task, filter.showDeclined) ?: return@mapNotNull null
        val conversation = task.conversationId?.let { state.conversations[it] }
        val project = task.projectId?.let { state.projects[it] }

        lane to BoardCard(task.id, task.title, task.agent, conversation?.model, project?.id, project?.name, task.branch, task.nodeId, conversation, task)
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
