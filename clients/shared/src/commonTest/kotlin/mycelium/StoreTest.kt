package mycelium

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
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
    fun openingAConversationFetchesItsWholeHistoryFromTheStart() = runTest {
        val requested = mutableListOf<String>()
        val engine = MockEngine { request ->
            requested += request.url.toString()
            val body = if (request.url.encodedPath.endsWith("/events")) {
                """[{"conversation_id":"c1","seq":1,"kind":"user_message","payload":{}},{"conversation_id":"c1","seq":2,"kind":"turn_end","payload":{}}]"""
            } else {
                "[]"
            }

            respond(body, HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json"))
        }
        val store = Store(MyceliumApi("https://mycelium.test", null, engine), backgroundScope)

        store.open("c1")

        assertEquals("https://mycelium.test/api/v1/conversations/c1/events?after=0&limit=500", requested.first())
        assertEquals(listOf(1L, 2L), store.state.value.events.getValue("c1").map { it.seq })
    }

    @Test
    fun reconnectingFetchesTheOpenConversationsEventsAfterTheLastSeqItHolds() = runTest {
        val requested = mutableListOf<String>()
        val engine = MockEngine { request ->
            requested += request.url.toString()
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

        assertTrue(requested.any { it.contains("/conversations/c1/events?after=2&limit=500") })
        assertEquals(listOf(1L, 2L, 3L), store.state.value.events.getValue("c1").map { it.seq })
    }
}
