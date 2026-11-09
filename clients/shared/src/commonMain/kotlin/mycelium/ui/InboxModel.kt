package mycelium.ui

import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import kotlin.math.roundToLong
import kotlin.time.Instant
import mycelium.api.Activity
import mycelium.api.Assistant
import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.api.Project
import mycelium.api.Workflow
import mycelium.api.WorkflowKinds
import mycelium.state.WorkflowStepStatuses
import mycelium.state.AppState
import mycelium.state.MyceliumCall
import mycelium.state.Store
import mycelium.state.ThreadItem
import mycelium.state.buildThread
import mycelium.state.myceliumCall
import mycelium.state.projectName

sealed interface InboxRequest {
    data class Approval(val permission: ThreadItem.Permission) : InboxRequest

    data class Mycelium(val call: ThreadItem.ToolCall, val parsed: MyceliumCall) : InboxRequest

    data class Gate(val workflow: Workflow) : InboxRequest

    data class Flag(val text: String) : InboxRequest
}

data class InboxItem(
    val key: String,
    val conversation: Conversation,
    val project: Project?,
    val projectName: String,
    val assistant: Assistant?,
    val waitingSince: String,
    val context: String?,
    val request: InboxRequest,
    val thread: List<ThreadItem>,
)

fun InboxItem.resolved(): InboxItem? = when (request) {
    is InboxRequest.Approval -> copy(request = InboxRequest.Approval(request.permission.copy(resolved = true)))
    is InboxRequest.Mycelium, is InboxRequest.Gate -> this
    is InboxRequest.Flag -> null
}

fun inboxConversations(conversations: Collection<Conversation>): List<Conversation> =
    conversations.filter { it.archivedAt == null && ((it.status == ConversationStatus.Input && it.managedBy == null) || it.flaggedAt != null) }

fun pendingRequest(item: ThreadItem, state: AppState): InboxRequest? {
    if (item is ThreadItem.Permission) {
        return if (item.resolved) null else InboxRequest.Approval(item)
    }

    if (item !is ThreadItem.ToolCall) {
        return null
    }

    val parsed = myceliumCall(item) ?: return null
    val waiting = when (parsed) {
        is MyceliumCall.SendToConversation -> proposedMessage(parsed, state.messageProposals)?.waiting == true
        is MyceliumCall.StartProposed -> proposedStart(parsed, state.startProposals)?.waiting == true
        is MyceliumCall.RequestApiAccess -> requestedAccess(parsed, state.apiAccessRequests)?.waiting == true
        is MyceliumCall.RequestSshAccess -> requestedSshAccess(parsed, state.sshAccessRequests)?.waiting == true
        else -> false
    }

    return if (waiting) InboxRequest.Mycelium(item, parsed) else null
}

data class PendingRequest(val key: String, val context: String?, val request: InboxRequest)

fun pendingRequests(thread: List<ThreadItem>, state: AppState): List<PendingRequest> =
    thread.mapIndexedNotNull { index, item ->
        val request = pendingRequest(item, state) ?: return@mapIndexedNotNull null

        PendingRequest(item.key, thread.take(index).filterIsInstance<ThreadItem.AgentMessage>().lastOrNull()?.text, request)
    }

fun workflowGates(state: AppState): List<Workflow> =
    state.workflows.values.filter { it.kind == WorkflowKinds.Mycelium && it.steps.any { step -> step.status == WorkflowStepStatuses.Gate } }

fun ensureOpened(store: Store, conversationId: String) {
    if (conversationId !in store.state.value.openedConversations) {
        store.act { store.open(conversationId) }
    }
}

fun inboxItems(state: AppState): List<InboxItem> {
    val fromConversations = inboxConversations(state.conversations.values).flatMap { conversation ->
        val thread = buildThread(state.events[conversation.id].orEmpty())
        val pending = pendingRequests(thread, state)
        val project = conversation.projectId?.let { state.projects[it] }
        val assistant = conversation.assistantId?.let { state.assistants[it] }
        val waitingSince = conversation.flaggedAt ?: conversation.updatedAt

        fun item(key: String, context: String?, request: InboxRequest) =
            InboxItem("${conversation.id}/$key", conversation, project, project?.name ?: projectName(conversation.cwd), assistant, waitingSince, context, request, thread)

        if (pending.isEmpty()) {
            val text = conversation.flagReason?.takeIf { it.isNotBlank() } ?: thread.filterIsInstance<ThreadItem.AgentMessage>().lastOrNull()?.text.orEmpty()

            listOf(item("flag", null, InboxRequest.Flag(text)))
        } else {
            pending.map { item(it.key, it.context, it.request) }
        }
    }
    val fromGates = workflowGates(state)
        .mapNotNull { workflow ->
            val conversation = workflow.conversationId?.let { state.conversations[it] } ?: return@mapNotNull null
            val project = conversation.projectId?.let { state.projects[it] }

            InboxItem("${conversation.id}/workflow-${workflow.id}", conversation, project, project?.name ?: projectName(conversation.cwd), conversation.assistantId?.let { state.assistants[it] }, workflow.updatedAt, null, InboxRequest.Gate(workflow), emptyList())
        }

    return (fromConversations + fromGates).sortedByDescending { it.waitingSince }
}

@Composable
fun rememberInbox(store: Store, state: AppState): List<InboxItem> {
    val waiting = inboxConversations(state.conversations.values).map { it.id }.sorted()

    LaunchedEffect(waiting) {
        for (id in waiting) {
            ensureOpened(store, id)
        }
    }

    return remember(state.conversations, state.events, state.messageProposals, state.startProposals, state.apiAccessRequests, state.sshAccessRequests, state.workflows, state.projects, state.assistants) { inboxItems(state) }
}

data class TokenSample(val at: Instant, val total: Long, val conversations: Map<String, Long>)

data class TokenRate(val perSecond: Long, val peak: Long, val today: Long, val lastHour: Long, val hourly: List<Long>, val running: Map<String, Long>)

fun tokenSample(activity: Activity, at: Instant) = TokenSample(at, activity.totals.tokens, activity.conversations.associate { it.key to it.tokens })

fun tokenRate(previous: TokenSample?, sample: TokenSample, activity: Activity, earlierPeak: Long): TokenRate {
    val seconds = previous?.let { (sample.at - it.at).inWholeMilliseconds / 1000.0 }?.takeIf { it > 0 }

    fun perSecond(now: Long, before: Long?): Long {
        if (seconds == null || before == null) {
            return 0
        }

        return ((now - before) / seconds).roundToLong().coerceAtLeast(0)
    }

    val hourly = activity.hours.map { it.tokens }
    val perSecond = if (previous == null) (hourly.lastOrNull() ?: 0) / SecondsPerHour else perSecond(sample.total, previous.total)
    val peak = maxOf(earlierPeak, perSecond, (hourly.maxOrNull() ?: 0) / SecondsPerHour)
    val running = if (previous == null) emptyMap() else sample.conversations.mapValues { (id, tokens) -> perSecond(tokens, previous.conversations[id] ?: 0) }

    return TokenRate(perSecond, peak, sample.total, hourly.lastOrNull() ?: 0, hourly, running)
}

private const val SecondsPerHour = 3600
