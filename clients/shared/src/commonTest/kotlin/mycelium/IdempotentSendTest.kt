package mycelium

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpStatusCode
import kotlinx.coroutines.CompletableDeferred
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
        withTimeout(5_000) { while (bodies.value.isEmpty()) delay(10) }
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
        withTimeout(5_000) { while (bodies.value.size < 2) delay(10) }

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
        withTimeout(5_000) { while (bodies.value.size < 1) delay(10) }
        withContext(Dispatchers.Default) { delay(100) }
        store.send("c1", "yes")
        withTimeout(5_000) { while (bodies.value.size < 2) delay(10) }

        val ids = bodies.value.map { clientId.find(it)!!.groupValues[1] }
        assertNotEquals(ids[0], ids[1])
    }
}
