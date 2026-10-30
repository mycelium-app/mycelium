package mycelium

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeout
import mycelium.api.MyceliumApi
import mycelium.api.StreamMessage
import mycelium.state.Store
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class StoreConnectionTest {
    private val conversation = """{"id":"c1","owner":"u","node_id":"n","agent":"claude","cwd":"/work","title":"Restore","status":"done","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z"}"""
    private val json = headersOf(HttpHeaders.ContentType, "application/json")

    private class Sockets {
        var opened = 0

        fun stream(onOpen: suspend () -> Unit): Flow<StreamMessage> = flow {
            opened += 1
            onOpen()
            awaitCancellation()
        }
    }

    private fun TestScope.store(sockets: Sockets, route: (String) -> String?): Store {
        val engine = MockEngine { request ->
            val body = route(request.url.encodedPath)

            if (body == null) {
                respond("", HttpStatusCode.InternalServerError)
            } else {
                respond(body, HttpStatusCode.OK, json)
            }
        }

        return Store(MyceliumApi("https://mycelium.test", null, engine), backgroundScope, stream = sockets::stream)
    }

    private suspend fun Store.untilConnected() = withContext(Dispatchers.Default) { withTimeout(5_000) { state.first { it.connected } } }

    @Test
    fun connectedDoesNotWaitForASlowConversationList() = runTest {
        val release = CompletableDeferred<Unit>()
        val sockets = Sockets()
        val engine = MockEngine { request ->
            if (request.url.encodedPath.endsWith("/conversations")) {
                release.await()
                respond("[$conversation]", HttpStatusCode.OK, json)
            } else {
                respond("[]", HttpStatusCode.OK, json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", null, engine), backgroundScope, stream = sockets::stream)

        store.start()
        val connected = store.untilConnected()

        assertTrue(connected.conversations.isEmpty())

        release.complete(Unit)
        val listed = withContext(Dispatchers.Default) { withTimeout(5_000) { store.state.first { it.conversations.isNotEmpty() } } }

        assertEquals(listOf("c1"), listed.conversations.keys.toList())
        assertTrue(listed.connected)
    }

    @Test
    fun aFailingWellnessCallDoesNotDropTheConnection() = runTest {
        val sockets = Sockets()
        val store = store(sockets) { path ->
            when {
                path.contains("/wellness") -> null
                path.endsWith("/conversations") -> "[$conversation]"
                else -> "[]"
            }
        }

        store.start()
        withContext(Dispatchers.Default) { withTimeout(5_000) { store.state.first { it.conversations.isNotEmpty() } } }

        assertTrue(store.state.value.connected)
        assertEquals(1, sockets.opened)
    }

    @Test
    fun resumingWhileConnectedLeavesTheSocketAlone() = runTest {
        val sockets = Sockets()
        val store = store(sockets) { "[]" }

        store.start()
        store.untilConnected()
        store.reconnect()
        withContext(Dispatchers.Default) { delay(100) }

        assertEquals(1, sockets.opened)
        assertTrue(store.state.value.connected)
    }
}
