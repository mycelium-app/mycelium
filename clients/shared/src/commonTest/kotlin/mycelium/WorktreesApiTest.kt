package mycelium

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.test.runTest
import mycelium.api.ApiException
import mycelium.api.CreateConversationRequest
import mycelium.api.MyceliumApi
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith

private val Json = headersOf(HttpHeaders.ContentType, "application/json")

class WorktreesApiTest {
    @Test
    fun aConversationInANewWorktreeSendsTheProjectAndBranch() = runTest {
        var body = ""
        val engine = MockEngine { request ->
            body = String(request.body.toByteArray())
            respond("{}", HttpStatusCode.BadRequest, Json)
        }

        runCatching { MyceliumApi("https://mycelium.test", null, engine).createConversation(CreateConversationRequest("n1", "claude", projectId = "p1", branch = "feature/x")) }

        assertEquals("""{"node_id":"n1","agent":"claude","project_id":"p1","branch":"feature/x"}""", body)
    }

    @Test
    fun aBranchGitRefusesFailsWithGitsMessage() = runTest {
        val engine = MockEngine { respond("""{"error":"git_failed","message":"a branch named 'x' already exists"}""", HttpStatusCode.Conflict, Json) }

        val failure = assertFailsWith<ApiException> { MyceliumApi("https://mycelium.test", null, engine).createConversation(CreateConversationRequest("n1", "claude", projectId = "p1", branch = "x")) }

        assertEquals("a branch named 'x' already exists", failure.message)
    }
}
