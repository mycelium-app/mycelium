package mycelium

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.withContext
import mycelium.api.MyceliumApi
import mycelium.state.Store
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotEquals

class IdempotentSendTest {
    private val clientId = Regex("\"client_id\":\"([^\"]+)\"")

    @Test
    fun aDoubleTapOnSendDispatchesOneRequest() = runTest {
        val bodies = MutableStateFlow(emptyList<String>())
        val answer = CompletableDeferred<Unit>()
        val engine = MockEngine { request ->
            bodies.update { it + String(request.body.toByteArray()) }
            answer.await()

            respond("", HttpStatusCode.NoContent)
        }
        val store = Store(MyceliumApi("https://mycelium.test", null, engine), backgroundScope)

        store.send("c1", "ship it")
        store.send("c1", "ship it")
        bodies.first { it.isNotEmpty() }
        withContext(Dispatchers.Default) { delay(100) }
        answer.complete(Unit)

        assertEquals(1, bodies.value.size)
    }

    @Test
    fun aRetryAfterADroppedConnectionReusesTheClientId() = runTest {
        val bodies = MutableStateFlow(emptyList<String>())
        val engine = MockEngine { request ->
            bodies.update { it + String(request.body.toByteArray()) }

            if (bodies.value.size == 1) {
                throw Exception("connection dropped")
            }

            respond("", HttpStatusCode.NoContent)
        }
        val store = Store(MyceliumApi("https://mycelium.test", null, engine), backgroundScope)

        store.send("c1", "ship it")
        bodies.first { it.size >= 2 }

        val ids = bodies.value.map { clientId.find(it)!!.groupValues[1] }
        assertEquals(ids[0], ids[1])
    }

    @Test
    fun twoSeparateSendsOfTheSameWordsGetDifferentClientIds() = runTest {
        val bodies = MutableStateFlow(emptyList<String>())
        val engine = MockEngine { request ->
            bodies.update { it + String(request.body.toByteArray()) }

            respond("", HttpStatusCode.NoContent)
        }
        val store = Store(MyceliumApi("https://mycelium.test", null, engine), backgroundScope)

        store.send("c1", "yes")
        bodies.first { it.size >= 1 }
        withContext(Dispatchers.Default) { delay(100) }
        store.send("c1", "yes")
        bodies.first { it.size >= 2 }

        val ids = bodies.value.map { clientId.find(it)!!.groupValues[1] }
        assertNotEquals(ids[0], ids[1])
    }

    @Test
    fun aVoiceSendRetriedAfterADroppedConnectionReusesTheClientId() = runTest {
        val bodies = MutableStateFlow(emptyList<String>())
        val engine = MockEngine { request ->
            if (request.url.encodedPath.endsWith("/assistants/builtin/voice")) {
                return@MockEngine respond("""{"id":"a1","name":"Voice","created_at":"2026-01-01T00:00:00Z"}""", HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json"))
            }

            bodies.update { it + String(request.body.toByteArray()) }

            if (bodies.value.size == 1) {
                throw Exception("connection dropped")
            }

            respond("", HttpStatusCode.NoContent)
        }
        val store = Store(MyceliumApi("https://mycelium.test", null, engine), backgroundScope)

        store.voice.say("turn the lights off")
        bodies.first { it.size >= 2 }

        val ids = bodies.value.map { clientId.find(it)!!.groupValues[1] }
        assertEquals(ids[0], ids[1])
    }
}
