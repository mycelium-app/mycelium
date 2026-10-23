package mycelium

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.test.runTest
import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.api.MyceliumApi
import mycelium.state.sideChatOf
import mycelium.state.sidebarRows
import mycelium.state.sidebarSections
import kotlin.test.Test
import kotlin.test.assertEquals

private fun chat(id: String, parentId: String? = null, status: ConversationStatus = ConversationStatus.Done, archivedAt: String? = null, updatedAt: String = "2026-10-22T10:00:00Z") =
    Conversation(id, "u", "n", "claude", "/work", "Title $id", status, createdAt = "2026-10-22T10:00:00Z", updatedAt = updatedAt, archivedAt = archivedAt, parentId = parentId, sideKind = parentId?.let { "side" })

class SideChatsTest {
    @Test
    fun theThreeCommandsSplitIntoTheirKindAndTheRestOfTheMessage() {
        assertEquals("btw" to "what does this default to?", sideChatOf("/btw what does this default to?"))
        assertEquals("side" to "", sideChatOf("/side"))
        assertEquals("fork" to "now do Android\nand iOS", sideChatOf("  /fork   now do Android\nand iOS "))
    }

    @Test
    fun anythingElseIsAnOrdinaryMessage() {
        assertEquals(null, sideChatOf("/compact"))
        assertEquals(null, sideChatOf("/btwo thanks"))
        assertEquals(null, sideChatOf("please /btw this"))
    }

    @Test
    fun aSideChatLeavesTheSectionsAndSitsUnderItsParent() {
        val all = listOf(chat("parent", status = ConversationStatus.Working), chat("side", "parent"))

        assertEquals(listOf("Working"), sidebarSections(all).map { it.name })
        assertEquals(listOf("parent" to 0, "side" to 1), sidebarRows(sidebarSections(all).single().rows, all, emptySet()).map { it.conversation.id to it.depth })
    }

    @Test
    fun aSideChatWhoseParentIsArchivedStandsOnItsOwn() {
        val all = listOf(chat("parent", archivedAt = "2026-10-22T11:00:00Z"), chat("side", "parent", ConversationStatus.Input))

        assertEquals(listOf("side"), sidebarSections(all).flatMap { it.rows }.map { it.id })
    }

    @Test
    fun collapsingAParentHidesItsSideChatsAtEveryDepthAndCountsThem() {
        val all = listOf(chat("parent"), chat("side", "parent"), chat("fork", "side"))
        val parents = listOf(all[0])

        assertEquals(listOf(1), sidebarRows(parents, all, setOf("parent")).map { it.sideChats })
        assertEquals(listOf("parent", "side", "fork"), sidebarRows(parents, all, emptySet()).map { it.conversation.id })
        assertEquals(listOf("parent"), sidebarRows(parents, all, setOf("parent")).map { it.conversation.id })
    }

    @Test
    fun startingASideChatPostsItsKindAndTextToTheConversation() = runTest {
        val seen = mutableListOf<Pair<String, String>>()
        val engine = MockEngine { request ->
            seen += "${request.method.value} ${request.url.encodedPath}" to String(request.body.toByteArray())

            respond("""{"id":"s1","owner":"u","node_id":"n","agent":"claude","cwd":"/work","title":"Side chat","status":"done","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","parent_id":"c1","side_kind":"btw"}""", HttpStatusCode.Created, headersOf(HttpHeaders.ContentType, "application/json"))
        }

        val started = MyceliumApi("https://mycelium.test", null, engine).startSideChat("c1", "btw", "What is the default?")

        assertEquals(listOf("${HttpMethod.Post.value} /api/v1/conversations/c1/side" to """{"kind":"btw","text":"What is the default?"}"""), seen)
        assertEquals("c1" to "btw", started.parentId to started.sideKind)
    }
}
