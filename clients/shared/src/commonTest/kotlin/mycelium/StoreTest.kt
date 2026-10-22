package mycelium

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeout
import mycelium.api.MyceliumApi
import mycelium.state.Store
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class StoreTest {
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
    fun openingAConversationFetchesItsWholeHistoryFromTheStart() = runTest {
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

        assertEquals("https://mycelium.test/api/v1/conversations/c1/events?after=0&limit=500", requested.value.first())
        assertEquals(listOf(1L, 2L), store.state.value.events.getValue("c1").map { it.seq })
    }

    @Test
    fun reconnectingFetchesTheOpenConversationsEventsAfterTheLastSeqItHolds() = runTest {
        val requested = MutableStateFlow(emptyList<String>())
        val engine = MockEngine { request ->
            requested.update { it + request.url.toString() }
            val body = if (request.url.encodedPath.endsWith("/events") && request.url.parameters["after"] == "0") {
                """[{"conversation_id":"c1","seq":1,"kind":"user_message","payload":{}},{"conversation_id":"c1","seq":2,"kind":"turn_end","payload":{}}]"""
            } else if (request.url.encodedPath.endsWith("/events")) {
                """[{"conversation_id":"c1","seq":3,"kind":"turn_end","payload":{}}]"""
            } else {
                "[]"
            }

            respond(body, HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json"))
        }
        val store = Store(MyceliumApi("https://mycelium.test", null, engine), backgroundScope)
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
