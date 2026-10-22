package mycelium.state

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import mycelium.api.ApiException
import mycelium.api.MyceliumApi

private const val EventPageSize = 500
private const val FirstRetryMillis = 1_000L
private const val LongestRetryMillis = 15_000L

class Store(val api: MyceliumApi, private val scope: CoroutineScope) {
    private val mutableState = MutableStateFlow(AppState())
    private val mutableProblem = MutableStateFlow<String?>(null)
    private var streaming: Job? = null

    val state: StateFlow<AppState> = mutableState.asStateFlow()
    val problem: StateFlow<String?> = mutableProblem.asStateFlow()

    fun start() {
        if (streaming != null) {
            return
        }

        streaming = scope.launch {
            var retry = FirstRetryMillis

            while (true) {
                try {
                    refresh()
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
        var after = state.value.events[conversationId]?.lastOrNull()?.seq ?: 0

        while (true) {
            val page = api.events(conversationId, after, EventPageSize)
            mutableState.update { it.copy(events = it.events + (conversationId to mergeEvents(it.events[conversationId].orEmpty(), page))) }

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

    fun act(block: suspend () -> Unit) {
        scope.launch {
            mutableProblem.value = null

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
    }

    fun dismissProblem() {
        mutableProblem.value = null
    }
}
