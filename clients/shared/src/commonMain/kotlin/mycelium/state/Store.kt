package mycelium.state

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import mycelium.api.ApiException
import mycelium.api.Conversation
import mycelium.api.MyceliumApi
import mycelium.api.EventKinds
import kotlinx.serialization.json.JsonObject
import mycelium.api.PatchConversationRequest
import kotlin.io.encoding.Base64

private const val EventPageSize = 500
private const val FirstRetryMillis = 1_000L
private const val LongestRetryMillis = 15_000L

class Store(val api: MyceliumApi, private val scope: CoroutineScope) {
    private val mutableState = MutableStateFlow(AppState())
    private val mutableProblem = MutableStateFlow<String?>(null)
    private var streaming: Job? = null
    private val typed = Channel<Pair<String, String>>(Channel.UNLIMITED)

    val state: StateFlow<AppState> = mutableState.asStateFlow()
    val problem: StateFlow<String?> = mutableProblem.asStateFlow()

    init {
        scope.launch {
            var carried: Pair<String, String>? = null

            while (true) {
                val (terminalId, first) = carried ?: typed.receive()
                var text = first
                carried = null

                while (true) {
                    val next = typed.tryReceive().getOrNull() ?: break

                    if (next.first != terminalId) {
                        carried = next
                        break
                    }

                    text += next.second
                }

                guarded { api.terminalInput(terminalId, Base64.encode(text.encodeToByteArray())) }
            }
        }
    }

    fun start() {
        if (streaming != null) {
            return
        }

        streaming = scope.launch {
            var retry = FirstRetryMillis

            while (true) {
                try {
                    refresh()
                    state.value.openedConversations.forEach { open(it) }
                    mutableState.update { it.copy(connected = true) }
                    retry = FirstRetryMillis
                    api.stream().collect { message -> mutableState.update { reduce(it, message) } }
                } catch (cancelled: CancellationException) {
                    throw cancelled
                } catch (_: Exception) {
                    mutableState.update { it.copy(connected = false) }
                }

                mutableState.update { it.copy(connected = false) }
                delay(retry)
                retry = minOf(retry * 2, LongestRetryMillis)
            }
        }
    }

    fun stop() {
        streaming?.cancel()
        streaming = null
    }

    suspend fun refresh() {
        val conversations = api.conversations(archived = false)
        val nodes = api.nodes()

        mutableState.update { it.copy(conversations = conversations.associateBy { c -> c.id }, nodes = nodes.associateBy { n -> n.id }) }
    }

    suspend fun open(conversationId: String) {
        var after = if (conversationId in state.value.openedConversations) state.value.events[conversationId]?.lastOrNull()?.seq ?: 0 else 0
        mutableState.update { it.copy(openedConversations = it.openedConversations + conversationId) }

        while (true) {
            val page = api.events(conversationId, after, EventPageSize)
            mutableState.update {
                val delivered = page.filter { event -> event.kind == EventKinds.UserMessage }.mapNotNull { event -> (event.payload as? JsonObject)?.string("text") }
                val sending = delivered.fold(it.sending) { waiting, text -> waiting.withoutFirst(conversationId, text) }

                it.copy(events = it.events + (conversationId to mergeEvents(it.events[conversationId].orEmpty(), page)), sending = sending)
            }

            if (page.size < EventPageSize) {
                break
            }

            after = page.maxOf { it.seq }
        }

        val queue = api.queue(conversationId)
        mutableState.update { it.copy(queues = it.queues + (conversationId to queue.sortedBy { q -> q.position })) }

        val conversation = state.value.conversations[conversationId]

        if (conversation != null && conversation.status == mycelium.api.ConversationStatus.Review) {
            api.markOpened(conversationId)
        }
    }

    fun send(conversationId: String, text: String) {
        mutableState.update { it.copy(sending = it.sending + (conversationId to it.sending[conversationId].orEmpty() + text)) }

        act {
            try {
                api.sendPrompt(conversationId, text)
            } catch (failure: Exception) {
                mutableState.update { it.copy(sending = it.sending.withoutFirst(conversationId, text)) }
                throw failure
            }
        }
    }

    fun typeIntoTerminal(terminalId: String, text: String) {
        typed.trySend(terminalId to text)
    }

    fun stopTerminal(terminalId: String) {
        val exited = terminalId in state.value.exitedTerminals
        mutableState.update { it.copy(terminalOutput = it.terminalOutput - terminalId, exitedTerminals = it.exitedTerminals - terminalId) }

        if (!exited) {
            act { api.stopTerminal(terminalId) }
        }
    }

    fun rename(conversation: Conversation, title: String) {
        mutableState.update { it.copy(conversations = it.conversations + (conversation.id to conversation.copy(title = title))) }

        act {
            try {
                val renamed = api.patchConversation(conversation.id, PatchConversationRequest(title = title))
                mutableState.update { it.copy(conversations = it.conversations + (renamed.id to renamed)) }
            } catch (failure: Exception) {
                mutableState.update { it.copy(conversations = it.conversations + (conversation.id to conversation)) }
                throw failure
            }
        }
    }

    fun act(block: suspend () -> Unit) {
        scope.launch {
            mutableProblem.value = null
            guarded(block)
        }
    }

    private suspend fun guarded(block: suspend () -> Unit) {
        try {
            block()
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (failure: ApiException) {
            mutableProblem.value = failure.message
        } catch (_: Exception) {
            mutableProblem.value = "Could not reach the control plane. Check your connection and try again."
        }
    }

    fun dismissProblem() {
        mutableProblem.value = null
    }
}
