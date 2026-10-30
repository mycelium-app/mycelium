package mycelium

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeout
import mycelium.api.MyceliumApi
import mycelium.api.WalkthroughNoteRequest
import mycelium.api.parseStreamMessage
import mycelium.state.Store
import mycelium.state.hasOlderEvents
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class StoreTest {
    @Test
    fun walkthroughNotesStreamedFromAnotherDeviceReplaceTheLocalOnes() = runTest {
        val store = Store(MyceliumApi("https://mycelium.test", null, MockEngine { respond("", HttpStatusCode.NoContent) }), backgroundScope)
        val note = """{"id":"n1","step":0,"comment":"Check this","sent":false,"created_at":"2026-10-23T10:00:00Z"}"""

        store.receive(parseStreamMessage("""{"type":"walkthrough_notes","conversation_id":"c1","walkthrough_id":"w1","notes":[$note]}""")!!)

        assertEquals(listOf("Check this"), store.walkthroughNotes.value.getValue("c1").map { it.comment })

        store.receive(parseStreamMessage("""{"type":"walkthrough_notes","conversation_id":"c1","walkthrough_id":"w1","notes":[]}""")!!)

        assertEquals(emptyList(), store.walkthroughNotes.value.getValue("c1"))
    }

    @Test
    fun aNoteTheStreamDeliversBeforeItsReplyIsListedOnce() = runTest {
        val note = """{"id":"n1","step":0,"comment":"Stage this","sent":false,"created_at":"2026-10-23T10:00:00Z"}"""
        lateinit var store: Store
        val engine = MockEngine {
            store.receive(parseStreamMessage("""{"type":"walkthrough_notes","conversation_id":"c1","walkthrough_id":"w1","notes":[$note]}""")!!)

            respond(note, HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json"))
        }
        store = Store(MyceliumApi("https://mycelium.test", null, engine), backgroundScope)

        store.addWalkthroughNote("c1", "w1", WalkthroughNoteRequest(step = 0, comment = "Stage this"))

        withTimeout(5_000) { while (store.walkthroughNotes.value["c1"].isNullOrEmpty()) delay(10) }
        withContext(Dispatchers.Default) { delay(100) }
        assertEquals(listOf("n1"), store.walkthroughNotes.value.getValue("c1").map { it.id })
    }

    @Test
    fun typedTerminalInputReachesTheNodeInOrderAndIsBatched() = runTest {
        val posted = MutableStateFlow(emptyList<String>())
        val engine = MockEngine { request ->
            posted.update { it + "${request.url.encodedPath} ${String(request.body.toByteArray())}" }

            respond("", HttpStatusCode.NoContent)
        }
        val store = Store(MyceliumApi("https://mycelium.test", null, engine), backgroundScope)

        store.typeIntoTerminal("t1", "l")
        store.typeIntoTerminal("t1", "s")
        store.typeIntoTerminal("t1", "\r")
        store.typeIntoTerminal("t2", "x")

        withTimeout(5_000) { while (posted.value.size < 2) delay(10) }
        val joined = posted.value.joinToString("|")

        assertEquals("/api/v1/terminals/t1/input {\"data\":\"bHMN\"}|/api/v1/terminals/t2/input {\"data\":\"eA==\"}", joined)
    }

    @Test
    fun stoppingATerminalTellsTheNodeToDetach() = runTest {
        val requested = MutableStateFlow(emptyList<String>())
        val engine = MockEngine { request ->
            requested.update { it + "${request.method.value} ${request.url.encodedPath}" }

            respond("", HttpStatusCode.NoContent)
        }
        val store = Store(MyceliumApi("https://mycelium.test", null, engine), backgroundScope)

        store.stopTerminal("t1")

        withTimeout(5_000) { while (requested.value.isEmpty()) delay(10) }
        assertEquals(listOf("DELETE /api/v1/terminals/t1"), requested.value)
    }

    @Test
    fun openingAConversationFetchesOnlyItsNewestPage() = runTest {
        val requested = MutableStateFlow(emptyList<String>())
        val engine = MockEngine { request ->
            requested.update { it + request.url.toString() }
            val body = if (request.url.encodedPath.endsWith("/events")) {
                """[{"conversation_id":"c1","seq":1,"kind":"user_message","payload":{}},{"conversation_id":"c1","seq":2,"kind":"turn_end","payload":{}}]"""
            } else {
                "[]"
            }

            respond(body, HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json"))
        }
        val store = Store(MyceliumApi("https://mycelium.test", null, engine), backgroundScope)

        store.open("c1")

        assertEquals(listOf("https://mycelium.test/api/v1/conversations/c1/events?before=${Long.MAX_VALUE}&limit=500"), requested.value.filter { "/events" in it })
        assertEquals(1, requested.value.count { it.contains("/events") })
        assertEquals(listOf(1L, 2L), store.state.value.events.getValue("c1").map { it.seq })
        assertFalse(store.state.value.hasOlderEvents("c1"))
    }

    @Test
    fun loadOlderPrependsThePreviousPageAndStopsWhenNoneRemain() = runTest {
        val requested = MutableStateFlow(emptyList<String>())
        val engine = MockEngine { request ->
            requested.update { it + request.url.toString() }
            val before = request.url.parameters["before"]
            val seqs = when (before) {
                Long.MAX_VALUE.toString() -> 6..8
                "6" -> 3..5
                "3" -> emptyList()
                else -> emptyList()
            }
            val body = if (request.url.encodedPath.endsWith("/events")) {
                seqs.joinToString(",", "[", "]") { """{"conversation_id":"c1","seq":$it,"kind":"turn_end","payload":{}}""" }
            } else {
                "[]"
            }

            respond(body, HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json"))
        }
        val store = Store(MyceliumApi("https://mycelium.test", null, engine), backgroundScope)
        store.open("c1")

        assertTrue(store.state.value.hasOlderEvents("c1"))

        store.loadOlder("c1")

        assertEquals((3L..8L).toList(), store.state.value.events.getValue("c1").map { it.seq })
        assertTrue(requested.value.any { it.contains("/events?before=6&limit=500") })

        store.loadOlder("c1")

        assertFalse(store.state.value.hasOlderEvents("c1"))

        val fetches = requested.value.size
        store.loadOlder("c1")

        assertEquals(fetches, requested.value.size)
    }

    @Test
    fun loadOlderWhileAnotherLoadIsRunningFetchesNothingMore() = runTest {
        val requested = MutableStateFlow(emptyList<String>())
        val release = CompletableDeferred<Unit>()
        val engine = MockEngine { request ->
            requested.update { it + request.url.toString() }

            if (request.url.parameters["before"] == "6") {
                release.await()
            }

            val seqs = if (request.url.parameters["before"] == "6") 3..5 else 6..8
            val body = if (request.url.encodedPath.endsWith("/events")) {
                seqs.joinToString(",", "[", "]") { """{"conversation_id":"c1","seq":$it,"kind":"turn_end","payload":{}}""" }
            } else {
                "[]"
            }

            respond(body, HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json"))
        }
        val store = Store(MyceliumApi("https://mycelium.test", null, engine), backgroundScope)
        store.open("c1")

        val first = launch { store.loadOlder("c1") }
        withContext(Dispatchers.Default) { withTimeout(5_000) { while (requested.value.none { it.contains("before=6") }) delay(10) } }
        store.loadOlder("c1")
        release.complete(Unit)
        first.join()

        assertEquals(1, requested.value.count { it.contains("before=6") })
    }

    @Test
    fun reconnectingFetchesTheOpenConversationsEventsAfterTheLastSeqItHolds() = runTest {
        val requested = MutableStateFlow(emptyList<String>())
        val engine = MockEngine { request ->
            requested.update { it + request.url.toString() }
            val body = if (request.url.encodedPath.endsWith("/events") && request.url.parameters["before"] != null) {
                """[{"conversation_id":"c1","seq":1,"kind":"user_message","payload":{}},{"conversation_id":"c1","seq":2,"kind":"turn_end","payload":{}}]"""
            } else if (request.url.encodedPath.endsWith("/events")) {
                """[{"conversation_id":"c1","seq":3,"kind":"turn_end","payload":{}}]"""
            } else {
                "[]"
            }

            respond(body, HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json"))
        }
        val store = Store(MyceliumApi("https://mycelium.test", null, engine), backgroundScope, stream = { onOpen -> flow { onOpen(); awaitCancellation() } })
        store.open("c1")

        store.start()

        withContext(Dispatchers.Default) {
            withTimeout(5_000) {
                while (store.state.value.events["c1"]?.size != 3) {
                    delay(10)
                }
            }
        }

        assertTrue(requested.value.any { it.contains("/conversations/c1/events?after=2&limit=500") })
        assertEquals(listOf(1L, 2L, 3L), store.state.value.events.getValue("c1").map { it.seq })
    }

    @Test
    fun reconnectingBeforeTheStoreHasStartedDoesNotStartIt() = runTest {
        val fetches = MutableStateFlow(0)
        val engine = MockEngine {
            fetches.update { it + 1 }
            respond("[]", HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json"))
        }
        val store = Store(MyceliumApi("https://mycelium.test", null, engine), backgroundScope)

        store.reconnect()

        withContext(Dispatchers.Default) { delay(100) }
        assertEquals(0, fetches.value)
    }

    @Test
    fun aCreatedProjectIsHeldInTheStoreByItsId() = runTest {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine {
            respond("""{"id":"p1","node_id":"n1","name":"app","path":"/w/app","created_at":"2026-10-22T10:00:00Z"}""", HttpStatusCode.Created, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", null, engine), backgroundScope)

        store.createProject(mycelium.api.CreateProjectRequest("n1", path = "work/app"))

        assertEquals("app", store.state.value.projects["p1"]?.name)
    }
}
