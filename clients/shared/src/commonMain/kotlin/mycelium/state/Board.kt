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
    val groupId: String?,
    val conversation: Conversation?,
    val task: Task?,
    val waitingOn: List<Task> = emptyList(),
    val idleConversations: List<Conversation> = emptyList(),
    val sendTargets: List<Conversation> = emptyList(),
) {
    val details: String
        get() = when (task?.status) {
            TaskStatuses.Question -> "From ${task.sourceTitle ?: "a conversation that is gone"}"
            TaskStatuses.Proposed -> listOfNotNull(projectName, task.sourceTitle?.let { "from $it" }).joinToString(" · ")
            TaskStatuses.Open -> if (idleConversations.isNotEmpty() && waitingOn.isEmpty()) {
                listOfNotNull(projectName, "any idle conversation").joinToString(" · ")
            } else {
                listOfNotNull(agentName(agent), model, projectName, branch, waitsOnText(waitingOn.size)).joinToString(" · ")
            }
            else -> listOfNotNull(agentName(agent), model, projectName, branch, waitsOnText(waitingOn.size)).joinToString(" · ")
        }
}

private const val DoneCardLimit = 50

private val unfinishedStatuses = setOf(TaskStatuses.Question, TaskStatuses.Proposed, TaskStatuses.Open, TaskStatuses.Claimed)

private fun waitsOnText(count: Int): String? = if (count == 0) null else "waits on $count ${if (count == 1) "task" else "tasks"}"

fun waitingOn(task: Task, tasks: Map<String, Task>): List<Task> =
    if (task.status == TaskStatuses.Open) task.dependsOn.mapNotNull { tasks[it] }.filter { it.status in unfinishedStatuses } else emptyList()

fun idleConversationsFor(task: Task, state: AppState): List<Conversation> =
    if (task.status != TaskStatuses.Open) {
        emptyList()
    } else {
        state.conversations.values
            .filter { it.projectId == task.projectId && it.archivedAt == null && !it.isChat && it.acpSessionId != null }
            .filter { it.status == ConversationStatus.Review || it.status == ConversationStatus.Done }
            .filter { state.nodes[it.nodeId]?.online == true }
            .sortedByDescending { it.updatedAt }
    }

fun sendTargetsFor(task: Task, state: AppState): List<Conversation> =
    if (task.status != TaskStatuses.Open) {
        emptyList()
    } else {
        val ofProject = state.conversations.values
            .filter { it.projectId == task.projectId && !it.isChat && it.acpSessionId != null && it.status != ConversationStatus.Failed }
            .filter { state.nodes[it.nodeId]?.online == true }
        val chats = state.conversations.values.filter { it.isChat && it.status != ConversationStatus.Failed }

        (ofProject + chats).filter { it.archivedAt == null }.sortedByDescending { it.updatedAt }
    }

val Conversation.acceptsAMessageNow: Boolean get() = status == ConversationStatus.Review || status == ConversationStatus.Done

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

fun boardCards(state: AppState, filter: BoardFilter = BoardFilter(), group: String = AllGroups): Map<Lane, List<BoardCard>> {
    val claimedBy = state.tasks.values.mapNotNull { it.conversationId }.toSet()
    val fromConversations = state.conversations.values
        .filter { it.archivedAt == null && it.id !in claimedBy }
        .sortedByDescending { it.updatedAt }
        .map { conversation ->
            val worktree = state.worktrees.values.firstOrNull { it.path == conversation.cwd }
            val project = worktree?.let { state.projects[it.projectId] } ?: state.projects.values.firstOrNull { it.path == conversation.cwd }

            laneOf(conversation.status) to BoardCard(conversation.id, conversation.title, conversation.agent, conversation.model, project?.id, project?.name, worktree?.branch, conversation.nodeId, conversation.groupId, conversation, null)
        }
    val fromTasks = state.tasks.values.sortedBy { it.createdAt }.mapNotNull { task ->
        val lane = laneOf(task, filter.showDeclined) ?: return@mapNotNull null
        val conversation = task.conversationId?.let { state.conversations[it] }
        val project = task.projectId?.let { state.projects[it] }

        lane to BoardCard(task.id, task.title, task.agent, conversation?.model, project?.id, project?.name, task.branch, task.nodeId, project?.groupId, conversation, task, waitingOn(task, state.tasks), idleConversationsFor(task, state), sendTargetsFor(task, state))
    }
    val matching = (fromConversations + fromTasks).filter { (_, card) -> inGroup(card.groupId, group) && matches(card, filter) }
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
