package mycelium.desktop

import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.test.assertCountEquals
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
import kotlinx.coroutines.runBlocking
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.NewProjectDialog
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private const val AtlasId = "aaaaaaaa-0000-0000-0000-000000000001"
private const val OrionId = "aaaaaaaa-0000-0000-0000-000000000002"
private const val WillowId = "aaaaaaaa-0000-0000-0000-000000000003"

private fun nodeJson(id: String, name: String) = """{"id":"$id","name":"$name","hostname":"$name","os":"linux","arch":"x86_64","version":"0.1.0","online":true,"draining":false,"agents":[],"logins":[]}"""

private const val BackupsOnAtlas = """{"id":"p1","node_id":"$AtlasId","name":"backups","path":"/home/sam/projects/acme-org/backups","forge_id":"f1","repository":"acme-org/backups","group_id":"g1",
"checkouts":[{"id":"k1","node_id":"$AtlasId","path":"/home/sam/projects/acme-org/backups","created_at":"2026-10-22T10:00:00Z"}],"created_at":"2026-10-22T10:00:00Z"}"""

private const val BackupsOnAtlasAndOrion = """{"id":"p1","node_id":"$AtlasId","name":"backups","path":"/home/sam/projects/acme-org/backups","forge_id":"f1","repository":"acme-org/backups","group_id":"g1",
"checkouts":[{"id":"k1","node_id":"$AtlasId","path":"/home/sam/projects/acme-org/backups","created_at":"2026-10-22T10:00:00Z"},{"id":"k2","node_id":"$OrionId","path":"/srv/backups","created_at":"2026-10-23T10:00:00Z"}],
"checkout_added":true,"created_at":"2026-10-22T10:00:00Z"}"""

class NewProjectCheckoutTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val posts = CopyOnWriteArrayList<Pair<String, String>>()
    private val opened = CopyOnWriteArrayList<String>()
    private val created = CopyOnWriteArrayList<String>()

    private fun show() {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            if (request.method == HttpMethod.Post) {
                posts += path to String(request.body.toByteArray())
            }

            when {
                path.endsWith("/projects") && request.method == HttpMethod.Post -> respond(BackupsOnAtlasAndOrion, HttpStatusCode.OK, json)
                path.endsWith("/projects") -> respond("[$BackupsOnAtlas]", HttpStatusCode.OK, json)
                path.endsWith("/repositories") -> respond("""{"repositories":[{"forge_id":"f1","owner":"acme-org","name":"backups","full_name":"acme-org/backups","private":true,"default_branch":"main"}],"organisations":[],"failures":[]}""", HttpStatusCode.OK, json)
                path.endsWith("/groups") -> respond("""[{"id":"g1","name":"homelab","created_at":"2026-10-22T10:00:00Z"}]""", HttpStatusCode.OK, json)
                path.endsWith("/forges") -> respond("""[{"id":"f1","kind":"github","base_url":"https://github.com","account":"sam","token_hint":"wxyz","created_at":"2026-10-22T10:00:00Z"}]""", HttpStatusCode.OK, json)
                path.endsWith("/nodes") -> respond("[${nodeJson(AtlasId, "atlas")},${nodeJson(OrionId, "orion")},${nodeJson(WillowId, "willow")}]", HttpStatusCode.OK, json)
                path.endsWith("/conversations") -> respond("[]", HttpStatusCode.OK, json)
                else -> respond("{}", HttpStatusCode.OK, json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        runBlocking { store.refresh(); store.refreshProjects() }

        rule.setContent {
            MyceliumTheme(true) {
                val state by store.state.collectAsState()

                NewProjectDialog(store, state, onDismiss = {}, onCreated = { created += it.id }, onExisting = { opened += it.id })
            }
        }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("acme-org/backups").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("acme-org/backups").performClick()
    }

    @Test
    fun aRepositoryThatIsAlreadyAProjectShowsTheNoticeAndOneActionToAddACheckout() {
        show()

        rule.onNodeWithText("acme-org/backups is already a project, checked out on atlas. Its group, icon and test command carry over.").assertExists()
        rule.onNodeWithText("Add a checkout on orion").assertExists()
        rule.onAllNodesWithText("Clone and open").assertCountEquals(0)
        rule.onAllNodesWithText("Group").assertCountEquals(0)
    }

    @Test
    fun addingTheCheckoutSendsOnlyTheNodeAndRepositoryAndOpensTheExistingProject() {
        show()

        rule.onNodeWithText("Add a checkout on orion").performClick()

        rule.waitUntil(5_000) { opened.isNotEmpty() }
        assertEquals("""{"node_id":"$OrionId","forge_id":"f1","repository":"acme-org/backups"}""", posts.single().second)
        assertEquals(listOf("p1"), opened.toList())
        assertTrue(created.isEmpty())
    }

    @Test
    fun choosingANodeThatHasACheckoutOpensTheProjectWithoutAskingTheServer() {
        show()

        rule.onNodeWithContentDescription("Set up on atlas, already has a checkout").performClick()
        rule.onNodeWithText("Open backups").performClick()

        rule.waitUntil(5_000) { opened.isNotEmpty() }
        assertTrue(posts.isEmpty())
    }
}
