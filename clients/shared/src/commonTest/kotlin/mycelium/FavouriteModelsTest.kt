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
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeout
import mycelium.api.ModelProvider
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.setFavouriteModel
import kotlin.test.Test
import kotlin.test.assertEquals

private val Starred = ModelProvider(id = "p1", kind = "openai_compatible", name = "Ollama", models = listOf("a", "b"), createdAt = "2026-01-01T00:00:00Z")

class FavouriteModelsTest {
    @Test
    fun twoQuickStarsBothStickWhenTheFirstReplyArrivesLast() = runTest {
        val firstReply = CompletableDeferred<Unit>()
        val engine = MockEngine { request ->
            val first = request.body.toByteArray().decodeToString().contains("\"a\"")

            if (first) {
                firstReply.await()
            }

            val favourites = if (first) """["a"]""" else """["a","b"]"""

            respond("""{"id":"p1","kind":"openai_compatible","name":"Ollama","models":["a","b"],"created_at":"2026-01-01T00:00:00Z","favourites":$favourites}""", HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json"))
        }
        val store = Store(MyceliumApi("https://mycelium.test", null, engine), backgroundScope)
        val providers = MutableStateFlow(listOf(Starred))
        val changes = MutableStateFlow(0)
        val change: ((List<ModelProvider>) -> List<ModelProvider>) -> Unit = { transform ->
            providers.update(transform)
            changes.update { it + 1 }
        }

        store.setFavouriteModel(change, Starred, "a", true)
        store.setFavouriteModel(change, Starred, "b", true)
        withContext(Dispatchers.Default) { withTimeout(5_000) { while (changes.value < 3) delay(10) } }
        firstReply.complete(Unit)
        withContext(Dispatchers.Default) { withTimeout(5_000) { while (changes.value < 4) delay(10) } }

        assertEquals(setOf("a", "b"), providers.value.single().favourites.toSet())
    }
}
