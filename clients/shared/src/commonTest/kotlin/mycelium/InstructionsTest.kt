package mycelium

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.test.runTest
import mycelium.api.MyceliumApi
import mycelium.api.NodeSummary
import mycelium.api.SetInstructionSourceRequest
import mycelium.api.InstructionLayer
import mycelium.state.heldVersion
import mycelium.state.layerFile
import mycelium.state.olderSessions
import mycelium.state.shortVersion
import kotlin.test.Test
import kotlin.test.assertEquals

private fun node(online: Boolean = true, revision: String? = null, older: Int = 0) = NodeSummary(
    id = "n", name = "willow", hostname = "h", os = "linux", arch = "x86_64", version = "0.1.0",
    online = online, draining = false, instructionsRevision = revision, sessionsOnOlderInstructions = older,
)

class InstructionsTest {
    @Test
    fun aVersionIsShownAsItsFirstSevenCharactersLikeACommit() {
        assertEquals("afad776", shortVersion("afad7763c4d5e6f708192a3b4c5d6e7f80910203"))
    }

    @Test
    fun aNodeShowsTheVersionItHoldsOrThatItHoldsNothingYet() {
        assertEquals("85d49d3", heldVersion(node(revision = "85d49d3aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")))
        assertEquals("Nothing yet", heldVersion(node()))
    }

    @Test
    fun sessionsOnAnOlderVersionAreCountedAndAnOfflineNodeSaysItCatchesUp() {
        assertEquals("None", olderSessions(node(older = 0)))
        assertEquals("1, updates on its next message", olderSessions(node(older = 1)))
        assertEquals("2, update on their next message", olderSessions(node(older = 2)))
        assertEquals("Offline, catches up on reconnect", olderSessions(node(online = false, older = 2)))
    }

    @Test
    fun aLayerFileNamesTheRepositoryAndPathOrSaysItIsStoredInMycelium() {
        val layer = InstructionLayer("global", "Global", "AGENTS.md")

        assertEquals("agents-md / AGENTS.md", layerFile(layer, "slop/agents-md"))
        assertEquals("Stored in Mycelium", layerFile(InstructionLayer("global", "Global"), null))
    }

    @Test
    fun savingALayerPutsItsTextAndPullingPostsToTheSourceEndpoint() = runTest {
        val seen = mutableListOf<Triple<HttpMethod, String, String>>()
        val engine = MockEngine { request ->
            seen += Triple(request.method, request.url.encodedPath, request.body.toByteArray().decodeToString())

            val body = if (request.url.encodedPath.endsWith("/instructions/global")) """{"layer":"global","name":"Global","text":"# Rules"}""" else """{"layers":[]}"""

            respond(body, HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json"))
        }
        val api = MyceliumApi("https://mycelium.test", "token", engine)

        api.saveInstructions("global", "# Rules")
        api.pullInstructions()
        api.setInstructionSource(SetInstructionSourceRequest("f1", "slop/agents-md"))

        assertEquals(Triple(HttpMethod.Put, "/api/v1/instructions/global", """{"text":"# Rules"}"""), seen[0])
        assertEquals(HttpMethod.Post to "/api/v1/instructions/pull", seen[1].first to seen[1].second)
        assertEquals(HttpMethod.Put to "/api/v1/instruction-source", seen[2].first to seen[2].second)
    }

    @Test
    fun aProjectJoinsAGroupWithAPatchAndLeavesItWithANull() = runTest {
        val bodies = mutableListOf<String>()
        val engine = MockEngine { request ->
            bodies += request.body.toByteArray().decodeToString()

            respond("""{"id":"p1","node_id":"n","name":"app","path":"/p","group_id":null,"created_at":"2026-10-22T10:00:00Z"}""", HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json"))
        }
        val api = MyceliumApi("https://mycelium.test", "token", engine)

        api.setProjectGroup("p1", "g1")
        api.setProjectGroup("p1", null)

        assertEquals(listOf("""{"group_id":"g1"}""", "{}"), bodies)
    }

    @Test
    fun aFolderClaimIsPostedToTheGroupWithItsNodeAndDeletedByItsId() = runTest {
        val seen = mutableListOf<Triple<HttpMethod, String, String>>()
        val engine = MockEngine { request ->
            seen += Triple(request.method, request.url.encodedPath, request.body.toByteArray().decodeToString())

            respond("""{"id":"f1","node_id":"n1","prefix":"/srv"}""", HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json"))
        }
        val api = MyceliumApi("https://mycelium.test", "token", engine)

        val folder = api.addGroupFolder("g1", "/srv", "n1")
        api.removeGroupFolder("g1", "f1")

        assertEquals("n1", folder.nodeId)
        assertEquals(Triple(HttpMethod.Post, "/api/v1/groups/g1/folders", """{"prefix":"/srv","node_id":"n1"}"""), seen[0])
        assertEquals(HttpMethod.Delete to "/api/v1/groups/g1/folders/f1", seen[1].first to seen[1].second)
    }
}
