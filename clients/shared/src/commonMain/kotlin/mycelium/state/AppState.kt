package mycelium.state

import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import kotlinx.serialization.json.JsonObject
import mycelium.api.Event
import mycelium.api.EventKinds
import mycelium.api.NodeSummary
import mycelium.api.QueuedPrompt
import mycelium.api.StreamMessage

data class AppState(
    val conversations: Map<String, Conversation> = emptyMap(),
    val nodes: Map<String, NodeSummary> = emptyMap(),
    val events: Map<String, List<Event>> = emptyMap(),
    val openedConversations: Set<String> = emptySet(),
    val queues: Map<String, List<QueuedPrompt>> = emptyMap(),
    val loginOutput: Map<String, String> = emptyMap(),
    val finishedLogins: Map<String, Boolean> = emptyMap(),
    val sending: Map<String, List<String>> = emptyMap(),
    val connected: Boolean = false,
)

data class SidebarSection(val status: ConversationStatus, val name: String, val rows: List<Conversation>)

fun reduce(state: AppState, message: StreamMessage): AppState = when (message) {
    is StreamMessage.ConversationChanged ->
        state.copy(conversations = state.conversations + (message.conversation.id to message.conversation))

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

    is StreamMessage.NodeChanged -> state.copy(nodes = state.nodes + (message.node.id to message.node))

    is StreamMessage.NodeRemoved -> state.copy(nodes = state.nodes - message.nodeId)

    is StreamMessage.LoginOutput ->
        state.copy(loginOutput = state.loginOutput + (message.loginId to state.loginOutput[message.loginId].orEmpty() + message.data))

    is StreamMessage.LoginFinished ->
        state.copy(finishedLogins = state.finishedLogins + (message.loginId to message.ok))
}

fun mergeEvents(existing: List<Event>, incoming: List<Event>): List<Event> =
    (existing + incoming).associateBy { it.seq }.values.sortedBy { it.seq }

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

fun sidebarSections(conversations: Collection<Conversation>): List<SidebarSection> =
    conversations
        .filter { it.archivedAt == null }
        .groupBy { it.status }
        .map { (status, rows) -> SidebarSection(status, statusSectionName(status), rows.sortedByDescending { it.updatedAt }) }
        .sortedBy { statusRank(it.status) }

internal fun Map<String, List<String>>.withoutFirst(conversationId: String, text: String?): Map<String, List<String>> {
    val current = this[conversationId] ?: return this
    val index = current.indexOf(text)

    if (index < 0) {
        return this
    }

    return this + (conversationId to current.filterIndexed { position, _ -> position != index })
}
