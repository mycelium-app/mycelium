package mycelium.ui

import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import kotlin.math.roundToLong
import kotlin.time.Instant
import mycelium.api.Activity
import mycelium.api.Assistant
import mycelium.api.InstructionProposal
import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.api.Project
import mycelium.api.Workflow
import mycelium.api.WorkflowKinds
import mycelium.state.ConversationPlace
import mycelium.state.place
import mycelium.state.WorkflowStepStatuses
import mycelium.state.AppState
import mycelium.state.MyceliumCall
import mycelium.state.MyceliumTool
import mycelium.state.Store
import mycelium.state.VoiceAssistantKey
import mycelium.state.ThreadItem
import mycelium.state.ToolStatus
import mycelium.state.ThreadBuilder
import mycelium.state.myceliumCall
import mycelium.state.projectName

sealed interface InboxRequest {
    data class Approval(val permission: ThreadItem.Permission) : InboxRequest

    data class Mycelium(val call: ThreadItem.ToolCall, val parsed: MyceliumCall) : InboxRequest

    data class Instructions(val proposal: InstructionProposal) : InboxRequest

    data class Gate(val workflow: Workflow) : InboxRequest

    data class Flag(val text: String) : InboxRequest

    data class ActionItem(val item: mycelium.api.ActionItem, val done: Boolean = false, val succeeded: Boolean = false) : InboxRequest
}

data class InboxItem(
    val key: String,
    val conversation: Conversation,
    val project: Project?,
    val projectName: String?,
    val assistant: Assistant?,
    val waitingSince: String,
    val context: String?,
    val request: InboxRequest,
    val thread: List<ThreadItem>,
) {
    val name: String get() = assistant?.name ?: conversation.title
}

private val RunningOrSucceeded = setOf("running", "succeeded")

fun InboxItem.resolved(): InboxItem? = when (request) {
    is InboxRequest.Approval -> copy(request = InboxRequest.Approval(request.permission.copy(resolved = true)))
    is InboxRequest.Mycelium, is InboxRequest.Gate -> this
    is InboxRequest.Instructions, is InboxRequest.Flag -> null
    is InboxRequest.ActionItem -> copy(request = if (request.item.run?.status in RunningOrSucceeded) request.copy(succeeded = true) else request.copy(done = true))
}

private fun waitsOnPerson(conversation: Conversation) =
    (conversation.status == ConversationStatus.Input && conversation.managedBy == null) || conversation.flaggedAt != null

fun inboxConversations(state: AppState): List<Conversation> =
    state.conversations.values.filter { conversation ->
        val voice = state.assistants[conversation.assistantId]?.builtinKey == VoiceAssistantKey

        conversation.archivedAt == null && !voice && (waitsOnPerson(conversation) || conversation.actionItems.isNotEmpty())
    }

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
        is MyceliumCall.RequestArtifactAccess -> parsed.requestId?.let { state.artifactAccessRequests[it] }?.state == "requested"
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

private data class WaitingApproval(val kind: String, val id: String, val conversationId: String?, val createdAt: String, val request: InboxRequest, val reason: String? = null)

private fun approvalCall(id: String, parsed: MyceliumCall) =
    InboxRequest.Mycelium(ThreadItem.ToolCall("approval-$id", id, "mycelium ${parsed.tool.key}", "other", "", ToolStatus.Done, null, null), parsed)

private fun waitingApprovals(state: AppState): List<WaitingApproval> =
    state.startProposals.values.filter { it.waiting }.map { proposal ->
        val tool = when (proposal.kind) {
            "workflow" -> MyceliumTool.StartWorkflow
            "conversation" -> MyceliumTool.StartConversation
            else -> MyceliumTool.StartSubagent
        }

        WaitingApproval("start", proposal.id, proposal.conversationId, proposal.createdAt, approvalCall(proposal.id, MyceliumCall.StartProposed(tool, proposal.title, proposal.id)))
    } + state.messageProposals.values.filter { it.waiting }.map { proposal ->
        WaitingApproval("message", proposal.id, proposal.conversationId, proposal.createdAt, approvalCall(proposal.id, MyceliumCall.SendToConversation(proposal.targetTitle, proposal.message, proposal.reason, proposal.id)), proposal.reason)
    } + state.apiAccessRequests.values.filter { it.waiting }.map { request ->
        WaitingApproval("api-access", request.id, request.conversationId, request.createdAt, approvalCall(request.id, MyceliumCall.RequestApiAccess(request.scopes, request.hours, request.reason, request.id)))
    } + state.sshAccessRequests.values.filter { it.waiting }.map { request ->
        WaitingApproval("ssh-access", request.id, request.conversationId, request.createdAt, approvalCall(request.id, MyceliumCall.RequestSshAccess(request.nodeName, request.minutes, request.reason, request.id)))
    } + state.artifactAccessRequests.values.filter { it.state == "requested" }.map { request ->
        WaitingApproval("artifact-access", request.id, request.conversationId, request.createdAt, approvalCall(request.id, MyceliumCall.RequestArtifactAccess(request.artifactId, request.permission, request.reason, request.id)))
    } + state.instructionProposals.values.filter { it.waiting }.map { proposal ->
        WaitingApproval("instructions", proposal.id, proposal.conversationId, proposal.createdAt, InboxRequest.Instructions(proposal))
    }

fun inboxItems(state: AppState, builders: MutableMap<String, ThreadBuilder> = mutableMapOf()): List<InboxItem> {
    val threadOf = { id: String -> builders.getOrPut(id) { ThreadBuilder() }.build(state.events[id].orEmpty()) }

    fun item(conversation: Conversation, key: String, waitingSince: String, context: String?, request: InboxRequest, thread: List<ThreadItem>): InboxItem {
        val project = conversation.projectId?.let { state.projects[it] }

        return InboxItem("${conversation.id}/$key", conversation, project, project?.name ?: projectName(conversation.cwd).takeUnless { conversation.place == ConversationPlace.Scratch }, conversation.assistantId?.let { state.assistants[it] }, waitingSince, context, request, thread)
    }

    val fromApprovals = waitingApprovals(state).mapNotNull { approval ->
        val conversation = approval.conversationId?.let { state.conversations[it] }?.takeIf { it.archivedAt == null } ?: return@mapNotNull null
        val thread = threadOf(conversation.id)
        val context = approval.reason ?: thread.filterIsInstance<ThreadItem.AgentMessage>().lastOrNull()?.text

        item(conversation, "${approval.kind}-${approval.id}", approval.createdAt, context, approval.request, thread)
    }
    val approving = fromApprovals.map { it.conversation.id }.toSet()
    val fromConversations = inboxConversations(state).flatMap { conversation ->
        val thread = threadOf(conversation.id)
        val pending = pendingRequests(thread, state).filter { it.request is InboxRequest.Approval }
        val waitingSince = conversation.flaggedAt ?: conversation.updatedAt

        val waiting = if (!waitsOnPerson(conversation)) {
            emptyList()
        } else if (pending.isEmpty() && conversation.id !in approving) {
            val text = conversation.flagReason?.takeIf { it.isNotBlank() } ?: thread.filterIsInstance<ThreadItem.AgentMessage>().lastOrNull()?.text.orEmpty()

            listOf(item(conversation, "flag", waitingSince, null, InboxRequest.Flag(text), thread))
        } else {
            pending.map { item(conversation, it.key, waitingSince, it.context, it.request, thread) }
        }
        val actionItems = conversation.actionItems.map { item(conversation, "action-${it.id}", it.createdAt, null, InboxRequest.ActionItem(it), thread) }

        waiting + actionItems
    }
    val fromGates = workflowGates(state)
        .mapNotNull { workflow ->
            val conversation = workflow.conversationId?.let { state.conversations[it] } ?: return@mapNotNull null
            val project = conversation.projectId?.let { state.projects[it] }

            InboxItem("${conversation.id}/workflow-${workflow.id}", conversation, project, project?.name ?: projectName(conversation.cwd).takeUnless { conversation.place == ConversationPlace.Scratch }, conversation.assistantId?.let { state.assistants[it] }, workflow.updatedAt, null, InboxRequest.Gate(workflow), emptyList())
        }

    return (fromConversations + fromApprovals + fromGates).sortedByDescending { it.waitingSince }
}

@Composable
fun rememberInbox(store: Store, state: AppState): List<InboxItem> {
    val waiting = inboxConversations(state).map { it.id }.sorted()

    LaunchedEffect(waiting) {
        for (id in waiting) {
            ensureOpened(store, id)
        }
    }

    val builders = remember { mutableMapOf<String, ThreadBuilder>() }

    LaunchedEffect(store, state.connected) {
        attempt { store.refreshStartProposals() }
        attempt { store.refreshMessageProposals() }
        attempt { store.refreshApiAccessRequests() }
        attempt { store.refreshSshAccessRequests() }
        attempt { store.refreshArtifactAccessRequests() }
        attempt { store.refreshInstructionProposals() }
    }

    return remember(state.conversations, state.events, state.messageProposals, state.startProposals, state.apiAccessRequests, state.sshAccessRequests, state.artifactAccessRequests, state.instructionProposals, state.workflows, state.projects, state.assistants) { inboxItems(state, builders) }
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
