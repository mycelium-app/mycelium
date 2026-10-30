package mycelium.desktop

import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import java.util.concurrent.CopyOnWriteArrayList
import kotlinx.coroutines.delay
import kotlinx.coroutines.runBlocking
import mycelium.api.Conversation
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.IssuesScreen
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private const val LaptopId = "aaaaaaaa-0000-0000-0000-000000000001"
private const val AtlasId = "aaaaaaaa-0000-0000-0000-000000000002"

private const val NodesJson = """[
{"id":"$LaptopId","name":"laptop","hostname":"laptop","os":"linux","arch":"x86_64","version":"0.1.0","online":true,"draining":false,
"agents":[{"agent":"claude","version":"2.1.308","available":true}],"logins":[]},
{"id":"$AtlasId","name":"atlas","hostname":"atlas","os":"linux","arch":"x86_64","version":"0.1.0","online":true,"draining":false,
"agents":[{"agent":"codex","version":"0.1.0","available":true}],"logins":[]}]"""

private const val IssuesJson = """{"issues":[
{"forge_id":"f1","repository":"acme-org/acme-backups","number":412,"title":"Restore fails silently after a 503","url":"https://x/412","body":""},
{"forge_id":"f1","repository":"acme-org/acme-backups","number":398,"title":"Move restore copy into shared strings","url":"https://x/398","body":""},
{"forge_id":"f1","repository":"slop/mycelium","number":5,"title":"Show the branch in the header","url":"https://x/5","body":""}],"failures":[]}"""

private const val ProjectsJson = """[
{"id":"p1","node_id":"$LaptopId","name":"acme-backups","path":"/p/a","forge_id":"f1","repository":"acme-org/acme-backups","created_at":"2026-10-22T10:00:00Z"},
{"id":"p2","node_id":"$AtlasId","name":"acme-backups","path":"/p/b","forge_id":"f1","repository":"acme-org/acme-backups","created_at":"2026-10-22T10:00:00Z"},
{"id":"p3","node_id":"$LaptopId","name":"mycelium","path":"/p/c","forge_id":"f1","repository":"slop/mycelium","created_at":"2026-10-22T10:00:00Z"}]"""

private const val WorktreesJson = """[{"id":"w1","project_id":"p1","branch":"issue/398-restore-strings","path":"/w/a","issue_number":398,"created_at":"2026-10-22T10:00:00Z"}]"""

private const val ConversationJson = """{"id":"c1","owner":"u","node_id":"$LaptopId","agent":"claude","cwd":"/w/a","title":"#398 Move","status":"working",
"acp_session_id":null,"mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null}"""

class IssuesTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val posts = CopyOnWriteArrayList<Pair<String, String>>()
    private val issueQueries = CopyOnWriteArrayList<String?>()
    private var issues = IssuesJson
    private var workspaceAnswer: Pair<HttpStatusCode, String> = HttpStatusCode.Created to ConversationJson

    private fun store(): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            if (path.endsWith("/issues")) {
                delay(20)
                issueQueries += request.url.parameters["q"]
            }

            when {
                path.endsWith("/workspace") -> {
                    posts += path to String(request.body.toByteArray())
                    respond(workspaceAnswer.second, workspaceAnswer.first, json)
                }
                path.endsWith("/issues") -> respond(issues, HttpStatusCode.OK, json)
                path.endsWith("/projects") && request.method == HttpMethod.Get -> respond(ProjectsJson, HttpStatusCode.OK, json)
                path.endsWith("/worktrees") -> respond(WorktreesJson, HttpStatusCode.OK, json)
                path.endsWith("/nodes") -> respond(NodesJson, HttpStatusCode.OK, json)
                path.endsWith("/conversations") -> respond("[]", HttpStatusCode.OK, json)
                else -> respond("{}", HttpStatusCode.OK, json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        runBlocking { store.refresh(); store.refreshProjects() }

        return store
    }

    private fun show(store: Store, waitFor: String = "Restore fails silently after a 503", onOpened: (Conversation) -> Unit = {}) {
        rule.setContent {
            MyceliumTheme(true) {
                val state by store.state.collectAsState()

                IssuesScreen(store, state, onOpened)
            }
        }
        rule.waitUntil(5_000) { rule.onAllNodesWithText(waitFor).fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun anIssueTitleOpensItsDescriptionInAPreviewAndCanStartAWorkspace() {
        issues = """{"issues":[{"forge_id":"f1","repository":"slop/mycelium","number":5,"title":"Show the branch in the header","url":"https://example.org/issues/5","body":"Show the current branch beside the title."}],"failures":[]}"""
        var opened: Conversation? = null
        show(store(), waitFor = "Show the branch in the header", onOpened = { opened = it })

        rule.onNodeWithContentDescription("Preview issue 5").performClick()
        rule.onNodeWithText("Show the current branch beside the title.").assertExists()
        rule.onNode(hasText("Create workspace") and !hasContentDescription("Create a workspace for issue 5")).performClick()

        rule.waitUntil(5_000) { opened != null }
        assertEquals("/api/v1/projects/p3/issues/5/workspace", posts.single().first)
    }

    @Test
    fun anIssueWithAWorktreeOffersToOpenItAndOneWithoutOffersToCreateOne() {
        show(store())

        rule.onNodeWithContentDescription("Open the workspace for issue 398").assertExists()
        rule.onNodeWithContentDescription("Create a workspace for issue 412").assertExists()
    }

    @Test
    fun theLinkedPullRequestsShowTheirStateAndAnIssueTheForgeCouldNotAnswerForIsUnknown() {
        issues = """{"issues":[
{"forge_id":"f1","repository":"acme-org/acme-backups","number":412,"title":"Restore fails silently after a 503","url":"https://x/412","pull_requests":[]},
{"forge_id":"f1","repository":"acme-org/acme-backups","number":398,"title":"Move restore copy into shared strings","url":"https://x/398","pull_requests":[{"number":77,"label":"MR !77","url":"https://x/77","state":"open"},{"number":70,"label":"MR !70","url":"https://x/70","state":"merged"}]},
{"forge_id":"f1","repository":"slop/mycelium","number":5,"title":"Show the branch in the header","url":"https://x/5"}],"failures":[]}"""
        show(store())

        rule.onNodeWithText("Pull request").assertExists()
        rule.onNodeWithText("MR !77 open").assertExists()
        rule.onNodeWithText("MR !70 merged").assertExists()
        rule.onAllNodesWithText("Unknown").fetchSemanticsNodes().let { assertEquals(1, it.size) }
    }

    @Test
    fun repositoryChipsFilterTheTable() {
        show(store())

        rule.onNodeWithContentDescription("Show only slop/mycelium, 1 issues").performClick()

        rule.onNodeWithText("Show the branch in the header").assertExists()
        assertEquals(0, rule.onAllNodesWithText("Restore fails silently after a 503").fetchSemanticsNodes().size)
    }

    @Test
    fun creatingAWorkspaceForARepositoryOnOneNodeStartsItAndOpensTheConversation() {
        val store = store()
        var opened: Conversation? = null
        show(store, onOpened = { opened = it })

        rule.onNodeWithContentDescription("Create a workspace for issue 5").performClick()

        rule.waitUntil(5_000) { opened != null }
        assertEquals("/api/v1/projects/p3/issues/5/workspace" to """{"agent":"claude"}""", posts.single())
    }

    @Test
    fun aRepositoryOnSeveralNodesAsksWhichOneAndUsesThatNodesAgent() {
        val store = store()
        var opened: Conversation? = null
        show(store, onOpened = { opened = it })

        rule.onNodeWithContentDescription("Create a workspace for issue 412").performClick()
        rule.onNodeWithText("atlas").performClick()

        rule.waitUntil(5_000) { opened != null }
        assertEquals("/api/v1/projects/p2/issues/412/workspace" to """{"agent":"codex"}""", posts.single())
    }

    @Test
    fun openingAnExistingWorkspaceStartsItOnTheNodeHoldingTheWorktree() {
        val store = store()
        var opened: Conversation? = null
        show(store, onOpened = { opened = it })

        rule.onNodeWithContentDescription("Open the workspace for issue 398").performClick()

        rule.waitUntil(5_000) { opened != null }
        assertEquals("/api/v1/projects/p1/issues/398/workspace", posts.single().first)
    }

    @Test
    fun aForgeFailureInTheListingIsShownWithItsMessage() {
        issues = """{"issues":[{"forge_id":"f1","repository":"slop/mycelium","number":5,"title":"Show the branch in the header","url":"u","body":""}],"failures":[{"forge_id":"f2","message":"GitLab could not be reached."}]}"""
        show(store(), waitFor = "Show the branch in the header")

        rule.waitUntil(5_000) { rule.onAllNodesWithText("GitLab could not be reached.").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun aRefusedWorkspaceShowsTheServersMessage() {
        workspaceAnswer = HttpStatusCode.Conflict to """{"error":"git_failed","message":"fatal: worktree path exists"}"""
        val store = store()
        show(store)

        rule.onNodeWithContentDescription("Create a workspace for issue 5").performClick()

        rule.waitUntil(5_000) { store.problem.value != null }
        assertEquals("fatal: worktree path exists", store.problem.value)
    }
}
