package mycelium

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeout
import mycelium.api.MyceliumApi
import mycelium.api.StreamMessage
import mycelium.state.Store
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

private fun conversationJson(id: String, parent: String? = null) =
    """{"id":"$id","owner":"u","node_id":"n","agent":"claude","cwd":"/work","title":"Chat $id","status":"review","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z"${parent?.let { ",\"parent_id\":\"$it\"" }.orEmpty()}}"""

private fun idle(onOpen: suspend () -> Unit): Flow<StreamMessage> = flow {
    onOpen()
    awaitCancellation()
}

class ConversationPagesTest {
    private val json = headersOf(HttpHeaders.ContentType, "application/json")

    @Test
    fun theFirstPageIsShownWithItsSettledCountAndTheNextPageLoadsOnRequestUntilThereIsNoMore() = runTest {
        val cursors = mutableListOf<String?>()
        val engine = MockEngine { request ->
            val body = when {
                request.url.encodedPath.endsWith("/conversations") && request.url.parameters["view"] == "sidebar" -> {
                    val cursor = request.url.parameters["cursor"]
                    cursors += cursor

                    if (cursor == null) """{"conversations":[${conversationJson("c1")}],"next":"page-2","counts":{"settled":7}}""" else """{"conversations":[${conversationJson("c2")}],"next":null}"""
                }
                else -> "[]"
            }
            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", null, engine), backgroundScope, stream = ::idle)

        store.start()
        val first = withContext(Dispatchers.Default) { withTimeout(5_000) { store.state.first { it.conversations.isNotEmpty() } } }
        store.loadMoreConversations()
        val both = withContext(Dispatchers.Default) { withTimeout(5_000) { store.state.first { it.conversations.size == 2 } } }
        store.loadMoreConversations()
        withContext(Dispatchers.Default) { delay(200) }

        assertEquals(setOf("c1"), first.conversations.keys)
        assertEquals(7, first.settledCount)
        assertEquals(setOf("c1", "c2"), both.conversations.keys)
        assertNull(both.nextConversations)
        assertEquals(listOf(null, "page-2"), cursors)
    }

    @Test
    fun aConversationOpenedByIdBringsItsParentsAndSurvivesTheNextRefresh() = runTest {
        val engine = MockEngine { request ->
            val body = when (request.url.encodedPath) {
                "/api/v1/conversations/c9" -> conversationJson("c9", parent = "c8")
                "/api/v1/conversations/c8" -> conversationJson("c8")
                "/api/v1/conversations" -> """{"conversations":[${conversationJson("c1")}],"next":null}"""
                else -> "[]"
            }
            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", null, engine), backgroundScope, stream = ::idle)

        store.loadConversationWithAncestors("c9")
        store.refresh()

        assertEquals(setOf("c1", "c8", "c9"), store.state.value.conversations.keys)
    }

    @Test
    fun anOlderControlPlaneThatAnswersWithTheWholeListHasNoFurtherPages() = runTest {
        val engine = MockEngine { request ->
            respond(if (request.url.encodedPath.endsWith("/conversations")) "[${conversationJson("c1")},${conversationJson("c2")}]" else "[]", HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", null, engine), backgroundScope, stream = ::idle)

        store.start()
        val loaded = withContext(Dispatchers.Default) { withTimeout(5_000) { store.state.first { it.conversations.size == 2 } } }

        assertNull(loaded.nextConversations)
    }

    @Test
    fun theLastListIsShownFromTheCacheBeforeTheNetworkAnswers() = runTest {
        val platform = KeepingPlatform()
        val engine = MockEngine { request ->
            respond(if (request.url.encodedPath.endsWith("/conversations")) """{"conversations":[${conversationJson("c1")}],"next":null}""" else "[]", HttpStatusCode.OK, json)
        }
        val first = Store(MyceliumApi("https://mycelium.test", null, engine), backgroundScope, platform, stream = ::idle)
        first.start()
        withContext(Dispatchers.Default) { withTimeout(5_000) { first.state.first { it.conversations.isNotEmpty() } } }
        first.stop()

        val offline = MockEngine { respond("", HttpStatusCode.ServiceUnavailable) }
        val next = Store(MyceliumApi("https://mycelium.test", null, offline), backgroundScope, platform, stream = ::idle)

        assertTrue("c1" in next.state.value.conversations, "${next.state.value.conversations.keys}")
    }

    @Test
    fun theArchiveIsAskedForAPageAtATimeAndAnOlderServersWholeArchiveIsOnePage() = runTest {
        val asked = mutableListOf<String>()
        val paged = MockEngine { request ->
            asked += "${request.url.parameters["view"]} ${request.url.parameters["archived"]} ${request.url.parameters["cursor"]}"
            respond("""{"conversations":[${conversationJson("a1")}],"next":"page-2"}""", HttpStatusCode.OK, json)
        }
        val older = MockEngine { respond("[${conversationJson("a1")},${conversationJson("a2")}]", HttpStatusCode.OK, json) }

        val first = MyceliumApi("https://mycelium.test", null, paged).archivedPage()
        MyceliumApi("https://mycelium.test", null, paged).archivedPage("page-2")
        val whole = MyceliumApi("https://mycelium.test", null, older).archivedPage()

        assertEquals(listOf("archive true null", "archive true page-2"), asked)
        assertEquals("page-2", first.next)
        assertEquals(listOf("a1", "a2"), whole.conversations.map { it.id })
        assertNull(whole.next)
    }
}
