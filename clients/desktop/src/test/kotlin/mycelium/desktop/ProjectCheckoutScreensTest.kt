package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.runBlocking
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.LocalTouch
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Shell
import org.junit.Rule
import kotlin.test.Test

private const val A = "aaaaaaaa-0000-0000-0000-000000000001"
private const val O = "aaaaaaaa-0000-0000-0000-000000000002"
private const val W = "aaaaaaaa-0000-0000-0000-000000000003"

private fun node(id: String, name: String, icon: String) = """{"id":"$id","name":"$name","hostname":"$name","os":"linux","arch":"x86_64","version":"0.1.0","online":true,"draining":false,"icon":"$icon",
"agents":[{"agent":"claude","version":"2.1.308","available":true}],"logins":[]}"""

private fun checkout(node: String, path: String) = """{"id":"k-$node-${path.hashCode()}","node_id":"$node","path":"$path","created_at":"2026-10-22T10:00:00Z"}"""

private fun project(id: String, name: String, repository: String?, forge: String?, group: String?, vararg places: Pair<String, String>) =
    """{"id":"$id","node_id":"${places.first().first}","name":"$name","path":"${places.first().second}","forge_id":${forge?.let { "\"$it\"" }},"repository":${repository?.let { "\"$it\"" }},"group_id":${group?.let { "\"$it\"" }},
"test_command":${if (id == "p1") "\"make test\"" else "null"},"checkouts":[${places.joinToString(",") { checkout(it.first, it.second) }}],"created_at":"2026-10-22T10:00:00Z"}"""

private val ProjectsJson = "[" + listOf(
    project("p1", "backups", "acme-org/backups", "f1", "g1", A to "/home/sam/projects/acme-org/backups", O to "/home/sam/projects/acme-org/backups"),
    project("p2", "website", "acme-org/website", "f1", "g2", A to "/home/sam/website", O to "/home/sam/website", W to "/home/sam/website"),
    project("p3", "ledger", "acme-org/ledger", "f2", "g2", A to "/home/sam/ledger"),
    project("p4", "notes", null, null, null, W to "/home/sam/notes"),
).joinToString(",") + "]"

private val BackupsOnAtlasOnlyJson = "[" + listOf(
    project("p1", "backups", "acme-org/backups", "f1", "g1", A to "/home/sam/projects/acme-org/backups"),
    project("p2", "website", "acme-org/website", "f1", "g2", A to "/home/sam/website", O to "/home/sam/website", W to "/home/sam/website"),
    project("p3", "ledger", "acme-org/ledger", "f2", "g2", A to "/home/sam/ledger"),
    project("p4", "notes", null, null, null, W to "/home/sam/notes"),
).joinToString(",") + "]"

private fun conversation(id: String, node: String, cwd: String, title: String, updated: String, branch: String) =
    """{"id":"$id","owner":"u","node_id":"$node","agent":"claude","cwd":"$cwd","title":"$title","status":"working","project_id":"p1","branch":"$branch",
"acp_session_id":null,"mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"$updated","archived_at":null}"""

private val ConversationsJson = "[" + listOf(
    conversation("c1", A, "/home/sam/projects/acme-org/backups", "Prune snapshots older than 90 days", "2026-10-22T11:00:00Z", "main"),
    conversation("c2", O, "/home/sam/projects/acme-org/backups", "Restore drill", "2026-10-22T12:00:00Z", "restore-drill"),
).joinToString(",") + "]"

private const val RepositoriesJson = """{"repositories":[
{"forge_id":"f1","owner":"acme-org","name":"backups","full_name":"acme-org/backups","private":true,"default_branch":"main"},
{"forge_id":"f1","owner":"acme-org","name":"website","full_name":"acme-org/website","private":true,"default_branch":"main"},
{"forge_id":"f2","owner":"acme-org","name":"ledger","full_name":"acme-org/ledger","private":true,"default_branch":"main"},
{"forge_id":"f1","owner":"acme-org","name":"status-page","full_name":"acme-org/status-page","private":true,"default_branch":"main"}],"organisations":[],"failures":[]}"""

private const val ForgesJson = """[{"id":"f1","kind":"github","base_url":"https://github.com","account":"sam","token_hint":"wxyz","organisations":["acme-org"],"created_at":"2026-10-22T10:00:00Z"},
{"id":"f2","kind":"forgejo","base_url":"https://git.example.com","account":"sam","token_hint":"abcd","organisations":["acme-org"],"created_at":"2026-10-22T10:00:00Z"}]"""

private const val GroupsJson = """[{"id":"g1","name":"homelab","created_at":"2026-10-22T10:00:00Z"},{"id":"g2","name":"work","created_at":"2026-10-22T10:00:00Z"}]"""

class ProjectCheckoutScreensTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private fun show(route: Route, projects: String = ProjectsJson) {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val nodes = "[${node(A, "atlas", "server")},${node(O, "orion", "server")},${node(W, "willow", "laptop")}]"
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            val body = when {
                path.endsWith("/projects") -> projects
                path.endsWith("/repositories") -> RepositoriesJson
                path.endsWith("/forges") -> ForgesJson
                path.endsWith("/groups") -> GroupsJson
                path.endsWith("/nodes") -> nodes
                path.endsWith("/nodes/any") -> """{"node_id":"$O","name":"orion"}"""
                path.endsWith("/conversations") && request.url.parameters["archived"] == "true" -> "[]"
                path.endsWith("/conversations") -> ConversationsJson
                path.endsWith("/queue") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        runBlocking { store.refresh(); store.refreshProjects() }

        rule.setContent {
            MyceliumTheme(true) {
                CompositionLocalProvider(LocalTouch provides false) {
                    Box(Modifier.size(1440.dp, 900.dp)) {
                        Shell(store, PlatformStub, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = route)
                    }
                }
            }
        }
        rule.waitForIdle()
    }

    private fun capture(name: String) {
        rule.waitForIdle()
        rule.captureScreen(name)
    }

    @Test
    fun projectsList() {
        show(Route.Projects)
        rule.waitUntil(5_000) { rule.onAllNodesWithText("status-page").fetchSemanticsNodes().isNotEmpty() }
        capture("5-7-projects")
    }

    @Test
    fun projectPageWithTheAddMenu() {
        show(Route.Project("p1"))
        rule.onNodeWithText("Add a checkout").performClick()
        capture("5-36-project-page")
    }

    @Test
    fun removeACheckout() {
        show(Route.Project("p1"))
        rule.onNodeWithContentDescription("More for the checkout on orion").performClick()
        rule.onNodeWithText("Remove checkout").performClick()
        capture("5-37-remove-checkout")
    }

    @Test
    fun newProjectForARepositoryThatIsAlreadyAProject() {
        show(Route.Projects, BackupsOnAtlasOnlyJson)
        rule.onNodeWithText("New project").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("acme-org/backups").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("acme-org/backups").performClick()
        capture("5-38-new-project-existing")
    }

    @Test
    fun runOnMenuForAProjectOnSeveralNodes() {
        show(Route.NewIn("p1"))
        rule.onNodeWithContentDescription("Run on orion").performClick()
        capture("4-28-run-on-menu")
    }
}
