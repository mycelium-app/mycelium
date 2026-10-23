package mycelium.state

import kotlin.time.Instant
import mycelium.api.Artifact
import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import kotlinx.serialization.json.JsonObject
import mycelium.api.Event
import mycelium.api.Goal
import mycelium.api.Group
import mycelium.api.InstructionProposal
import mycelium.api.Memory
import mycelium.api.ApiAccessRequest
import mycelium.api.MessageProposal
import mycelium.api.MoveProgress
import mycelium.api.Walkthrough
import mycelium.api.EventKinds
import mycelium.api.CpuPoint
import mycelium.api.NodeResources
import mycelium.api.NodeSummary
import mycelium.api.Project
import mycelium.api.Repository
import mycelium.api.Schedule
import mycelium.api.ScheduleRun
import mycelium.api.Task
import mycelium.api.Worktree
import mycelium.api.QueuedPrompt
import mycelium.api.StreamMessage

const val TerminalChunkLimit = 1024
const val ResourceHistoryLimit = 200

data class TerminalFeed(val received: Int = 0, val chunks: List<String> = emptyList()) {
    fun unseen(consumed: Int): List<String> = chunks.takeLast((received - consumed).coerceIn(0, chunks.size))

    fun with(chunk: String): TerminalFeed = TerminalFeed(received + 1, (chunks + chunk).takeLast(TerminalChunkLimit))
}

data class AppState(
    val conversations: Map<String, Conversation> = emptyMap(),
    val nodes: Map<String, NodeSummary> = emptyMap(),
    val resources: Map<String, NodeResources> = emptyMap(),
    val projects: Map<String, Project> = emptyMap(),
    val groups: Map<String, Group> = emptyMap(),
    val worktrees: Map<String, Worktree> = emptyMap(),
    val tasks: Map<String, Task> = emptyMap(),
    val schedules: Map<String, Schedule> = emptyMap(),
    val artifacts: Map<String, Artifact> = emptyMap(),
    val scheduleRuns: List<ScheduleRun> = emptyList(),
    val instructionProposals: Map<String, InstructionProposal> = emptyMap(),
    val messageProposals: Map<String, MessageProposal> = emptyMap(),
    val apiAccessRequests: Map<String, ApiAccessRequest> = emptyMap(),
    val memories: Map<String, Memory> = emptyMap(),
    val removedMemories: Set<String> = emptySet(),
    val memoryRevision: Int = 0,
    val events: Map<String, List<Event>> = emptyMap(),
    val sessionUpdates: Map<String, List<JsonObject>> = emptyMap(),
    val openedConversations: Set<String> = emptySet(),
    val loadingOlderEvents: Set<String> = emptySet(),
    val completeHistories: Set<String> = emptySet(),
    val queues: Map<String, List<QueuedPrompt>> = emptyMap(),
    val moves: Map<String, MoveProgress> = emptyMap(),
    val goals: Map<String, Goal> = emptyMap(),
    val walkthroughs: Map<String, Walkthrough> = emptyMap(),
    val loginOutput: Map<String, String> = emptyMap(),
    val finishedLogins: Map<String, Boolean> = emptyMap(),
    val terminalOutput: Map<String, TerminalFeed> = emptyMap(),
    val exitedTerminals: Set<String> = emptySet(),
    val sending: Map<String, List<String>> = emptyMap(),
    val connected: Boolean = false,
)

data class SidebarSection(val status: ConversationStatus, val name: String, val rows: List<Conversation>)

fun reduce(state: AppState, message: StreamMessage): AppState = when (message) {
    is StreamMessage.ConversationChanged ->
        if (message.conversation.sideKind == GoalCheckKind) state else state.copy(conversations = state.conversations + (message.conversation.id to message.conversation))

    is StreamMessage.ConversationRemoved -> {
        val id = message.conversationId

        state.copy(
            conversations = state.conversations - id,
            events = state.events - id,
            sessionUpdates = state.sessionUpdates - id,
            openedConversations = state.openedConversations - id,
            loadingOlderEvents = state.loadingOlderEvents - id,
            completeHistories = state.completeHistories - id,
            queues = state.queues - id,
            goals = state.goals - id,
            walkthroughs = state.walkthroughs - id,
            moves = state.moves - id,
        )
    }

    is StreamMessage.EventAppended -> {
        val id = message.event.conversationId

        val sent = (message.event.payload as? JsonObject)?.string("text").takeIf { message.event.kind == EventKinds.UserMessage }
        val delivered = state.copy(sending = state.sending.withoutFirst(id, sent))

        if (id in state.openedConversations) {
            delivered.copy(events = state.events + (id to mergeEvents(state.events[id].orEmpty(), listOf(message.event))))
        } else {
            delivered
        }
    }

    is StreamMessage.QueueChanged -> {
        val queued = message.queue.map { it.text }
        val waiting = state.sending[message.conversationId].orEmpty().filterNot { it in queued }

        state.copy(
            queues = state.queues + (message.conversationId to message.queue.sortedBy { it.position }),
            sending = state.sending + (message.conversationId to waiting),
        )
    }

    is StreamMessage.GoalChanged -> state.copy(goals = state.goals + (message.goal.conversationId to message.goal))

    is StreamMessage.GoalCleared -> state.copy(goals = state.goals - message.conversationId)

    is StreamMessage.WalkthroughChanged -> state.copy(walkthroughs = state.walkthroughs + (message.walkthrough.conversationId to message.walkthrough))

    is StreamMessage.WalkthroughNotesChanged -> state

    is StreamMessage.MoveChanged -> state.copy(moves = state.moves + (message.progress.conversationId to message.progress))

    is StreamMessage.ScheduleChanged -> state.copy(schedules = state.schedules + (message.schedule.id to message.schedule))

    is StreamMessage.ScheduleRemoved -> state.copy(schedules = state.schedules - message.scheduleId)

    is StreamMessage.ArtifactChanged ->
        state.copy(artifacts = if (message.deleted) state.artifacts - message.artifact.id else state.artifacts + (message.artifact.id to message.artifact))

    is StreamMessage.ArtifactCommentAdded -> state

    is StreamMessage.InstructionProposalChanged -> state.copy(instructionProposals = state.instructionProposals + (message.proposal.id to message.proposal))

    is StreamMessage.MessageProposalChanged -> state.copy(messageProposals = state.messageProposals + (message.proposal.id to message.proposal))

    is StreamMessage.ApiAccessRequestChanged -> state.copy(apiAccessRequests = state.apiAccessRequests + (message.request.id to message.request))

    is StreamMessage.MemoryChanged -> state.copy(
        memories = state.memories + (message.memory.id to message.memory),
        removedMemories = state.removedMemories - message.memory.id,
        memoryRevision = state.memoryRevision + 1,
    )

    is StreamMessage.MemoryRemoved -> state.copy(
        memories = state.memories - message.memoryId,
        removedMemories = state.removedMemories + message.memoryId,
        memoryRevision = state.memoryRevision + 1,
    )

    is StreamMessage.NodeChanged -> state.copy(nodes = state.nodes + (message.node.id to message.node))

    is StreamMessage.NodeRemoved -> state.copy(nodes = state.nodes - message.nodeId, resources = state.resources - message.nodeId)

    is StreamMessage.NodeResourcesSampled -> {
        val held = state.resources[message.nodeId] ?: NodeResources()
        val point = CpuPoint(message.sample.atMs, message.sample.cpuPercent)
        val history = if (held.history.any { it.atMs == point.atMs }) held.history else (held.history + point).takeLast(ResourceHistoryLimit)

        state.copy(resources = state.resources + (message.nodeId to NodeResources(message.sample, history)))
    }

    is StreamMessage.LoginOutput ->
        state.copy(loginOutput = state.loginOutput + (message.loginId to state.loginOutput[message.loginId].orEmpty() + message.data))

    is StreamMessage.LoginFinished ->
        state.copy(finishedLogins = state.finishedLogins + (message.loginId to message.ok))

    is StreamMessage.TerminalOutput ->
        state.copy(terminalOutput = state.terminalOutput + (message.terminalId to (state.terminalOutput[message.terminalId] ?: TerminalFeed()).with(message.data)))

    is StreamMessage.TerminalExited ->
        state.copy(exitedTerminals = state.exitedTerminals + message.terminalId)
}

fun AppState.conversationTitle(id: String?): String? = id?.let { conversations[it]?.title }

fun AppState.hasOlderEvents(id: String): Boolean {
    val oldest = events[id]?.firstOrNull()?.seq ?: return false

    return id !in completeHistories && oldest > 1
}

fun projectsOfRepository(projects: Collection<Project>, repository: Repository): List<Project> =
    projects.filter { it.forgeId == repository.forgeId && it.repository == repository.fullName }

fun registeredFolders(projects: Collection<Project>): List<Project> = projects.filter { it.forgeId == null }.sortedBy { it.name }

fun lastActivity(project: Project, conversations: Collection<Conversation>): String =
    conversations.filter { it.projectId == project.id }.map { it.updatedAt }.maxByOrNull { runCatching { Instant.parse(it) }.getOrNull() ?: Instant.DISTANT_PAST } ?: project.createdAt

fun mergeEvents(existing: List<Event>, incoming: List<Event>): List<Event> {
    val appends = incoming.zipWithNext().all { (a, b) -> a.seq < b.seq } && (existing.isEmpty() || incoming.isEmpty() || existing.last().seq < incoming.first().seq)

    if (appends) {
        return existing + incoming
    }

    return (existing + incoming).associateBy { it.seq }.values.sortedBy { it.seq }
}

fun statusRank(status: ConversationStatus): Int = when (status) {
    ConversationStatus.Input -> 0
    ConversationStatus.Failed -> 1
    ConversationStatus.Review -> 2
    ConversationStatus.Working -> 3
    ConversationStatus.Done -> 4
}

fun statusSectionName(status: ConversationStatus): String = when (status) {
    ConversationStatus.Input -> "Needs input"
    ConversationStatus.Failed -> "Failed"
    ConversationStatus.Review -> "Ready to review"
    ConversationStatus.Working -> "Working"
    ConversationStatus.Done -> "Done"
}

const val AllGroups = "all"
const val NoGroup = "none"
const val PinnedName = "Pinned"

fun inGroupFilter(conversation: Conversation, filter: String): Boolean = when (filter) {
    AllGroups -> true
    NoGroup -> conversation.groupId == null
    else -> conversation.groupId == filter
}

private fun topLevel(conversations: Collection<Conversation>, filter: String): List<Conversation> {
    val active = conversations.filter { it.archivedAt == null && !it.isChat && inGroupFilter(it, filter) }
    val activeIds = active.map { it.id }.toSet()

    return active.filter { it.parentId == null || it.parentId !in activeIds }
}

fun pinnedConversations(conversations: Collection<Conversation>, filter: String = AllGroups): List<Conversation> =
    topLevel(conversations, filter).filter { it.pinnedAt != null }.sortedBy { it.pinnedAt }

fun sidebarSections(conversations: Collection<Conversation>, filter: String = AllGroups): List<SidebarSection> {
    return topLevel(conversations, filter)
        .filter { it.pinnedAt == null }
        .groupBy { if (it.resumeAt != null) ConversationStatus.Working else it.status }
        .map { (status, rows) -> SidebarSection(status, statusSectionName(status), rows.sortedByDescending { it.updatedAt }) }
        .sortedBy { statusRank(it.status) }
}

data class SidebarRow(val conversation: Conversation, val depth: Int, val sideChats: Int)

fun sidebarRows(parents: List<Conversation>, conversations: Collection<Conversation>, collapsed: Set<String>, depth: Int = 0): List<SidebarRow> =
    parents.flatMap { parent ->
        val children = conversations.filter { it.archivedAt == null && it.parentId == parent.id }
        val sideChats = children.filter { !it.isSubagent }.sortedByDescending { it.updatedAt } + children.filter { it.isSubagent }.sortedBy { it.createdAt }
        val shown = if (parent.id in collapsed) emptyList() else sidebarRows(sideChats, conversations, collapsed, depth + 1)

        listOf(SidebarRow(parent, depth, sideChats.size)) + shown
    }

internal fun Map<String, List<String>>.withoutFirst(conversationId: String, text: String?): Map<String, List<String>> {
    val current = this[conversationId] ?: return this
    val index = current.indexOf(text)

    if (index < 0) {
        return this
    }

    return this + (conversationId to current.filterIndexed { position, _ -> position != index })
}
