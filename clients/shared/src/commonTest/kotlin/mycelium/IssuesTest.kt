package mycelium

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.test.runTest
import mycelium.api.ApiException
import mycelium.api.Issue
import mycelium.api.MyceliumApi
import mycelium.api.Project
import mycelium.api.Worktree
import mycelium.state.issueCounts
import mycelium.state.projectsOfIssue
import mycelium.state.worktreesOfIssue
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith

private val Json = headersOf(HttpHeaders.ContentType, "application/json")

private fun issue(number: Long, repository: String = "acme-org/acme-backups", forge: String = "f1") = Issue(forge, repository, number, "Title $number", "https://x/$number")

private fun project(id: String, node: String, repository: String? = "acme-org/acme-backups", forge: String? = "f1") =
    Project(id, node, id, "/p/$id", forge, repository, "2026-10-22T10:00:00Z")

private fun worktree(id: String, project: String, number: Long?) = Worktree(id, project, "issue/$id", "/w/$id", number, "2026-10-22T10:00:00Z")

class IssuesTest {
    @Test
    fun anIssuesWorktreesAreFoundOnEveryNodeWhereTheRepositoryIsSetUp() {
        val projects = listOf(project("a", "n1"), project("b", "n2"), project("c", "n1", "other/repo"))
        val worktrees = listOf(worktree("w1", "a", 398), worktree("w2", "b", 398), worktree("w3", "a", 12), worktree("w4", "c", 398), worktree("w5", "a", null))

        assertEquals(listOf("w1", "w2"), worktreesOfIssue(projects, worktrees, issue(398)).map { it.id })
        assertEquals(emptyList(), worktreesOfIssue(projects, worktrees, issue(7)))
    }

    @Test
    fun anIssuesProjectsMatchOnForgeAndRepository() {
        val projects = listOf(project("a", "n1"), project("b", "n2", forge = "f2"), project("c", "n1", null, null))

        assertEquals(listOf("a"), projectsOfIssue(projects, issue(1)).map { it.id })
    }

    @Test
    fun repositoryCountsGroupIssuesByRepositoryInNameOrder() {
        val counts = issueCounts(listOf(issue(1), issue(2), issue(3, "slop/mycelium"), issue(4)))

        assertEquals(listOf("acme-org/acme-backups" to 3, "slop/mycelium" to 1), counts.map { it.repository to it.count })
    }

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

    @Test
    fun startingAWorkspacePostsTheAgentToTheProjectsIssue() = runTest {
        var seen = ""
        val engine = MockEngine { request ->
            seen = "${request.url.encodedPath} ${String(request.body.toByteArray())}"
            respond(
                """{"id":"c1","owner":"u","node_id":"n","agent":"claude","cwd":"/w","title":"#412 Restore fails","status":"working",
                "acp_session_id":null,"mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null}""",
                HttpStatusCode.Created,
                Json,
            )
        }

        val conversation = MyceliumApi("https://mycelium.test", null, engine).startWorkspace("p1", 412, "claude")

        assertEquals("/api/v1/projects/p1/issues/412/workspace {\"agent\":\"claude\"}", seen)
        assertEquals("#412 Restore fails", conversation.title)
    }

    @Test
    fun aProjectWithoutAForgeFailsWithTheServersSentence() = runTest {
        val engine = MockEngine { respond("""{"error":"project_without_forge","message":"This project did not come from a forge."}""", HttpStatusCode.Conflict, Json) }

        val failure = assertFailsWith<ApiException> { MyceliumApi("https://mycelium.test", null, engine).startWorkspace("p1", 1, "claude") }

        assertEquals("project_without_forge", failure.code)
    }
}
