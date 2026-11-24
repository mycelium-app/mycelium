package mycelium

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.test.runTest
import mycelium.api.MyceliumApi
import kotlin.test.Test
import kotlin.test.assertEquals

private val Json = headersOf(HttpHeaders.ContentType, "application/json")

class IssuesTest {
    @Test
    fun issuesDecodeWithFailuresAndSendTheTrimmedQuery() = runTest {
        var url = ""
        val engine = MockEngine { request ->
            url = request.url.toString()
            respond(
                """{"issues":[{"forge_id":"f1","repository":"acme-org/acme-backups","number":412,"title":"Restore fails","url":"https://x/412","body":null}],
                "failures":[{"forge_id":"f2","message":"GitLab could not be reached."}]}""",
                HttpStatusCode.OK,
                Json,
            )
        }

        val listing = MyceliumApi("https://mycelium.test", null, engine).issues(" is:open ")

        assertEquals(412L, listing.issues.single().number)
        assertEquals("GitLab could not be reached.", listing.failures.single().message)
        assertEquals("https://mycelium.test/api/v1/issues?q=is%3Aopen", url)
    }
}
