package mycelium

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpStatusCode
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.test.runTest
import mycelium.api.MyceliumApi
import mycelium.api.StreamMessage
import mycelium.state.Store
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private val ClientId = Regex("\"client_id\":\"([^\"]+)\"")

private fun prompts(bodies: List<String>) = bodies.filter { it.contains("\"text\"") }

private fun idle(onOpen: suspend () -> Unit): Flow<StreamMessage> = flow {
    onOpen()
    awaitCancellation()
}

class OutboxTest {
    @Test
    fun aMessageWhoseConnectionDropsMidSendStaysAsNotSent() = runTest {
        val engine = MockEngine { throw Exception("connection dropped") }
        val store = Store(MyceliumApi("https://mycelium.test", null, engine), backgroundScope)

        store.send("c1", "ship it")
        val unsent = store.state.first { it.outbox["c1"].orEmpty().any { message -> message.failure != null } }.outbox["c1"]!!.single()

        assertEquals("ship it" to "You are offline.", unsent.text to unsent.failure)
    }

    @Test
    fun anUnsentMessageIsStillThereAfterAReload() = runTest {
        val platform = KeepingPlatform()
        val engine = MockEngine { throw Exception("connection dropped") }
        val first = Store(MyceliumApi("https://mycelium.test", null, engine), backgroundScope, platform)

        first.send("c1", "ship it")
        first.state.first { it.outbox["c1"].orEmpty().any { message -> message.failure != null } }
        val reloaded = Store(MyceliumApi("https://mycelium.test", null, engine), backgroundScope, platform)

        assertEquals(listOf("ship it"), reloaded.state.value.outbox["c1"].orEmpty().map { it.text })
        assertTrue(reloaded.state.value.outbox["c1"]!!.single().failure != null)
    }

    @Test
    fun anUnsentMessageIsSentOnceWhenTheConnectionComesBack() = runTest {
        val online = MutableStateFlow(false)
        val bodies = MutableStateFlow(emptyList<String>())
        val engine = MockEngine { request ->
            if (!online.value) {
                throw Exception("connection dropped")
            }

            bodies.update { it + String(request.body.toByteArray()) }

            respond("", HttpStatusCode.NoContent)
        }
        val store = Store(MyceliumApi("https://mycelium.test", null, engine), backgroundScope, stream = ::idle)

        store.send("c1", "ship it")
        store.state.first { it.outbox["c1"].orEmpty().any { message -> message.failure != null } }
        online.value = true
        store.start()
        store.state.first { it.outbox["c1"].orEmpty().all { message -> message.failure == null } }
        bodies.first { prompts(it).isNotEmpty() }

        assertEquals(1, prompts(bodies.value).size)
    }

    @Test
    fun aRetryAfterASuccessTheClientNeverSawReusesTheClientId() = runTest {
        val bodies = MutableStateFlow(emptyList<String>())
        val engine = MockEngine { request ->
            bodies.update { it + String(request.body.toByteArray()) }

            if (prompts(bodies.value).size <= 2) {
                throw Exception("the answer never arrived")
            }

            respond("", HttpStatusCode.NoContent)
        }
        val store = Store(MyceliumApi("https://mycelium.test", null, engine), backgroundScope)

        store.send("c1", "ship it")
        val unsent = store.state.first { it.outbox["c1"].orEmpty().any { message -> message.failure != null } }.outbox["c1"]!!.single()
        store.retry("c1", unsent.clientId)
        bodies.first { prompts(it).size >= 3 }

        assertEquals(1, prompts(bodies.value).map { ClientId.find(it)!!.groupValues[1] }.toSet().size)
    }

    @Test
    fun deletingAnUnsentMessageRemovesItFromTheDevice() = runTest {
        val platform = KeepingPlatform()
        val engine = MockEngine { throw Exception("connection dropped") }
        val store = Store(MyceliumApi("https://mycelium.test", null, engine), backgroundScope, platform)

        store.send("c1", "ship it")
        val unsent = store.state.first { it.outbox["c1"].orEmpty().any { message -> message.failure != null } }.outbox["c1"]!!.single()
        store.discard("c1", unsent.clientId)
        val reloaded = Store(MyceliumApi("https://mycelium.test", null, engine), backgroundScope, platform)

        assertEquals(emptyList(), reloaded.state.value.outbox["c1"].orEmpty())
    }
}
