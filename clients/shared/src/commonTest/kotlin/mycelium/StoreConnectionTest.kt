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
import kotlinx.coroutines.launch
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeout
import kotlinx.coroutines.withTimeoutOrNull
import mycelium.api.MyceliumApi
import mycelium.api.FellBehindCloseCode
import mycelium.api.StreamClosed
import mycelium.api.StreamMessage
import mycelium.api.parseStreamMessage
import mycelium.state.Store
import kotlin.test.Test
import kotlin.time.Duration.Companion.seconds
import kotlin.time.TestTimeSource
import kotlin.time.TimeSource
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class StoreConnectionTest {
    private val conversation = """{"id":"c1","owner":"u","node_id":"n","agent":"claude","cwd":"/work","title":"Restore","status":"done","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z"}"""
    private val node = """{"id":"n","name":"Node","hostname":"h","os":"linux","arch":"x86_64","version":"1","online":true,"draining":false}"""
    private val json = headersOf(HttpHeaders.ContentType, "application/json")

    private class Sockets {
        var opened = 0

        fun stream(onOpen: suspend () -> Unit): Flow<StreamMessage> = flow {
            opened += 1
            onOpen()
            awaitCancellation()
        }
    }

    private fun TestScope.store(sockets: Sockets, clock: TimeSource = TimeSource.Monotonic, route: (String) -> String?): Store {
        val engine = MockEngine { request ->
            val body = route(request.url.encodedPath)

            if (body == null) {
                respond("", HttpStatusCode.InternalServerError)
            } else {
                respond(body, HttpStatusCode.OK, json)
            }
        }

        return Store(MyceliumApi("https://mycelium.test", null, engine), backgroundScope, stream = sockets::stream, timeSource = clock)
    }

    private suspend fun Store.untilConnected() = withContext(Dispatchers.Default) { withTimeout(5_000) { state.first { it.connected } } }

    private class ClosesFirst(private val code: Int?) {
        var opened = 0

        fun stream(onOpen: suspend () -> Unit): Flow<StreamMessage> = flow {
            opened += 1
            onOpen()

            if (opened == 1) {
                throw StreamClosed(code, "closed for the test")
            }

            awaitCancellation()
        }
    }

    @Test
    fun aStreamClosedForFallingBehindReopensWithoutShowingTheAppAsDisconnected() = runTest {
        val sockets = ClosesFirst(FellBehindCloseCode)
        val engine = MockEngine { respond("[]", HttpStatusCode.OK, json) }
        val store = Store(MyceliumApi("https://mycelium.test", null, engine), backgroundScope, stream = sockets::stream)
        val seen = mutableListOf<Boolean>()
        backgroundScope.launch(Dispatchers.Default) { store.state.collect { seen += it.connected } }

        store.start()
        repeat(100) {
            if (sockets.opened < 2) {
                advanceTimeBy(100)
                runCurrent()
            }
        }
        withContext(Dispatchers.Default) { delay(200) }

        assertEquals(2, sockets.opened)
        assertTrue(true in seen, "$seen")
        assertTrue(seen.dropWhile { !it }.all { it }, "$seen")
    }

    private val later = """{"id":"c2","owner":"u","node_id":"n","agent":"claude","cwd":"/work","title":"Missed while behind","status":"working","created_at":"2026-10-22T11:00:00Z","updated_at":"2026-10-22T11:00:00Z"}"""

    @Test
    fun aClientThatFellBehindEndsWithTheSameStateAsOneThatNeverLagged() = runTest {
        val sockets = ClosesFirst(FellBehindCloseCode)
        var listings = 0
        val engine = MockEngine { request ->
            val body = when {
                request.url.encodedPath.endsWith("/conversations") -> if (sockets.opened < 2) "[$conversation]" else "[$conversation,$later]".also { listings++ }
                else -> "[]"
            }
            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", null, engine), backgroundScope, stream = sockets::stream)
        val fresh = Store(MyceliumApi("https://mycelium.test", null, MockEngine { request -> respond(if (request.url.encodedPath.endsWith("/conversations")) "[$conversation,$later]" else "[]", HttpStatusCode.OK, json) }), backgroundScope, stream = Sockets()::stream)

        store.start()
        fresh.start()
        repeat(100) {
            if (sockets.opened < 2) {
                advanceTimeBy(100)
                runCurrent()
            }
        }
        val resynced = withContext(Dispatchers.Default) { withTimeoutOrNull(5_000) { store.state.first { it.conversations.size == 2 } } }
        val neverLagged = withContext(Dispatchers.Default) { withTimeoutOrNull(5_000) { fresh.state.first { it.conversations.size == 2 } } }
        assertTrue(resynced != null && neverLagged != null, "opened ${sockets.opened}, listings $listings, lagged ${store.state.value.conversations.keys} ${store.state.value.streamOpenings}, fresh ${fresh.state.value.conversations.keys}")

        assertEquals(neverLagged!!.conversations.keys, resynced!!.conversations.keys)
        assertEquals(2, resynced.streamOpenings)
        assertTrue(resynced.connected)
    }

    private class AlwaysFallsBehind {
        var opened = 0

        fun stream(onOpen: suspend () -> Unit): Flow<StreamMessage> = flow {
            opened += 1
            onOpen()
            throw StreamClosed(FellBehindCloseCode, "fell behind")
        }
    }

    @Test
    fun aClientThatKeepsFallingBehindBacksOffInsteadOfReopeningAtFullSpeed() = runTest {
        val sockets = AlwaysFallsBehind()
        val store = Store(MyceliumApi("https://mycelium.test", null, MockEngine { respond("[]", HttpStatusCode.OK, json) }), backgroundScope, stream = sockets::stream)

        store.start()
        advanceTimeBy(5_000)
        runCurrent()

        assertTrue(sockets.opened in 2..8, "reopened ${sockets.opened} times in 5 seconds")
        assertTrue(store.state.value.connected)
    }

    @Test
    fun theConversationListAndAnOpenedConversationReportHowLongTheyTookUnderTheRequestIdTheySent() = runTest {
        val requestIds = mutableMapOf<String, String?>()
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            requestIds[path.substringAfterLast("/")] = request.headers["x-request-id"]
            respond(if (path.endsWith("/conversations")) "[$conversation]" else "[]", HttpStatusCode.OK, json)
        }
        val reported = mutableListOf<Triple<String, String, Map<String, Any?>>>()
        val store = Store(MyceliumApi("https://mycelium.test", null, engine), backgroundScope, stream = Sockets()::stream, report = { kind, message, detail -> reported += Triple(kind, message, detail) })

        store.start()
        withContext(Dispatchers.Default) { withTimeout(5_000) { store.state.first { it.conversations.isNotEmpty() } } }
        runCatching { store.open("c1") }
        withContext(Dispatchers.Default) { withTimeout(5_000) { while (reported.none { it.second == "conversation opened" }) delay(10) } }

        val list = reported.first { it.second == "conversation list shown" }
        val opened = reported.first { it.second == "conversation opened" }
        assertEquals("timing", list.first)
        assertEquals(requestIds["conversations"], list.third["request_id"])
        assertTrue(list.third["ms"] is Long, "$list")
        assertEquals(1, list.third["conversations"])
        assertEquals(requestIds["events"], opened.third["request_id"])
        assertEquals(false, opened.third["reopened"])
        assertTrue(requestIds["events"]?.length == 36, "$requestIds")
    }

    @Test
    fun aLostStreamIsReportedWithItsCloseCodeAndHowLongItWasUp() = runTest {
        val sockets = ClosesFirst(1006)
        val engine = MockEngine { respond("[]", HttpStatusCode.OK, json) }
        val reported = mutableListOf<Triple<String, String, Map<String, Any?>>>()
        val store = Store(MyceliumApi("https://mycelium.test", null, engine), backgroundScope, stream = sockets::stream, report = { kind, message, detail -> reported += Triple(kind, message, detail) })

        store.start()
        withContext(Dispatchers.Default) { withTimeout(5_000) { while (sockets.opened < 2) delay(10) } }

        val closed = reported.first { it.second == "closed" }
        assertEquals("connection", closed.first)
        assertEquals(1006, closed.third["close_code"])
        assertEquals("closed for the test", closed.third["reason"])
        assertTrue(closed.third["up_for_ms"] is Long, "$closed")
        assertEquals(listOf("open", "closed", "open"), reported.filter { it.first == "connection" }.map { it.second })
    }

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

    @Test
    fun resumingAfterALongSilenceRestartsTheSocketAtOnce() = runTest {
        val sockets = Sockets()
        val clock = TestTimeSource()
        val store = store(sockets, clock) { "[]" }

        store.start()
        store.untilConnected()
        clock += 30.seconds
        store.reconnect()

        withContext(Dispatchers.Default) { withTimeout(5_000) { while (sockets.opened < 2) delay(10) } }

        assertEquals(2, sockets.opened)
    }

    @Test
    fun aConversationStreamedDuringTheFirstListingIsKept() = runTest {
        val release = CompletableDeferred<Unit>()
        val sockets = Sockets()
        val engine = MockEngine { request ->
            if (request.url.encodedPath.endsWith("/conversations")) {
                release.await()
            }

            val body = if (request.url.encodedPath.endsWith("/nodes")) """[$node]""" else "[]"

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", null, engine), backgroundScope, stream = sockets::stream)

        store.start()
        store.untilConnected()
        store.receive(parseStreamMessage("""{"type":"conversation",${conversation.removePrefix("{")}""")!!)
        release.complete(Unit)
        val listed = withContext(Dispatchers.Default) { withTimeout(5_000) { store.state.first { it.nodes.isNotEmpty() } } }

        assertEquals(listOf("c1"), listed.conversations.keys.toList())
    }
}
