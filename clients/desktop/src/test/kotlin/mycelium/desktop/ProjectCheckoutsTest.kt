package mycelium.desktop

import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
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
import kotlinx.coroutines.runBlocking
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.ProjectPage
import mycelium.ui.ProjectsScreen
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private const val AtlasNode = "aaaaaaaa-0000-0000-0000-000000000001"
private const val OrionNode = "aaaaaaaa-0000-0000-0000-000000000002"
private const val WillowNode = "aaaaaaaa-0000-0000-0000-000000000003"

private const val Nodes = """[
{"id":"$AtlasNode","name":"atlas","hostname":"atlas","os":"linux","arch":"x86_64","version":"0.1.0","online":true,"draining":false,"agents":[],"logins":[]},
{"id":"$OrionNode","name":"orion","hostname":"orion","os":"linux","arch":"x86_64","version":"0.1.0","online":true,"draining":false,"agents":[],"logins":[]},
{"id":"$WillowNode","name":"willow","hostname":"willow","os":"linux","arch":"x86_64","version":"0.1.0","online":true,"draining":false,"agents":[],"logins":[]}]"""

private const val Forges = """[{"id":"f1","kind":"github","base_url":"https://github.com","account":"sam","token_hint":"wxyz","organisations":["acme-org"],"created_at":"2026-10-22T10:00:00Z"}]"""

private const val Repositories = """{"repositories":[{"forge_id":"f1","owner":"acme-org","name":"backups","full_name":"acme-org/backups","private":true,"default_branch":"main"}],"organisations":[],"failures":[]}"""

private const val TwoCheckouts = """{"id":"p1","node_id":"$AtlasNode","name":"backups","path":"/home/sam/projects/acme-org/backups","forge_id":"f1","repository":"acme-org/backups","test_command":"make test",
"checkouts":[{"id":"k1","node_id":"$AtlasNode","path":"/home/sam/projects/acme-org/backups","created_at":"2026-10-22T10:00:00Z"},
{"id":"k2","node_id":"$OrionNode","path":"/srv/backups","created_at":"2026-10-23T10:00:00Z"}],"created_at":"2026-10-22T10:00:00Z"}"""

private const val WithoutCheckouts = """{"id":"p1","node_id":"$AtlasNode","name":"backups","path":"/home/sam/projects/acme-org/backups","forge_id":"f1","repository":"acme-org/backups","created_at":"2026-10-22T10:00:00Z"}"""

class ProjectCheckoutsTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val requests = java.util.concurrent.CopyOnWriteArrayList<Pair<String, String>>()
    private var removalAnswer = HttpStatusCode.NoContent to ""

    private fun store(project: String): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            if (request.method != HttpMethod.Get) {
                requests += "${request.method.value} $path" to String(request.body.toByteArray())
            }

            when {
                request.method == HttpMethod.Delete -> respond(removalAnswer.second, removalAnswer.first, json)
                path.endsWith("/projects") -> respond("[$project]", HttpStatusCode.OK, json)
                path.endsWith("/repositories") -> respond(Repositories, HttpStatusCode.OK, json)
                path.endsWith("/forges") -> respond(Forges, HttpStatusCode.OK, json)
                path.endsWith("/nodes") -> respond(Nodes, HttpStatusCode.OK, json)
                path.endsWith("/groups") || path.endsWith("/conversations") -> respond("[]", HttpStatusCode.OK, json)
                else -> respond("{}", HttpStatusCode.OK, json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        runBlocking { store.refresh() }

        return store
    }

    private fun showPage(store: Store) {
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); ProjectPage(store, state, "p1", onBack = {}, onNewConversation = {}) } }
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Checkout on atlas").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun theListShowsOneRowForARepositoryOnTwoNodes() {
        val store = store(TwoCheckouts)
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); ProjectsScreen(store, state, onNewConversation = {}) } }

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Checkout on orion").fetchSemanticsNodes().isNotEmpty() }
        rule.onAllNodesWithContentDescription("New conversation in acme-org/backups").assertCountEquals(1)
        rule.onNodeWithContentDescription("Checkout on atlas").assertExists()
        rule.onNodeWithContentDescription("Checkout on orion").assertExists()
    }

    @Test
    fun anOlderServersProjectWithoutCheckoutsStillShowsItsOneCheckout() {
        val store = store(WithoutCheckouts)
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); ProjectsScreen(store, state, onNewConversation = {}) } }

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Checkout on atlas").fetchSemanticsNodes().isNotEmpty() }
        rule.onAllNodesWithContentDescription("Checkout on orion").assertCountEquals(0)
    }

    @Test
    fun anOlderServersProjectPageListsTheOneCheckoutWithoutARemoveAction() {
        showPage(store(WithoutCheckouts))

        rule.onNodeWithContentDescription("Checkout on atlas").assertExists()
        rule.onAllNodesWithContentDescription("More for the checkout on atlas").assertCountEquals(0)
    }

    @Test
    fun theProjectPageListsBothCheckoutsWithTheirFolders() {
        showPage(store(TwoCheckouts))

        rule.onNodeWithContentDescription("Checkout on atlas").assertExists()
        rule.onNodeWithContentDescription("Checkout on orion").assertExists()
        rule.onNodeWithText("/srv/backups").assertExists()
        rule.onNodeWithText("make test").assertExists()
    }

    @Test
    fun removingACheckoutAsksFirstThenDeletesItOnItsNode() {
        showPage(store(TwoCheckouts))

        rule.onNodeWithContentDescription("More for the checkout on orion").performClick()
        rule.onNodeWithText("Remove checkout").performClick()
        rule.onNodeWithText("Remove the checkout on orion?").assertExists()
        assertTrue(requests.isEmpty())

        rule.onNodeWithText("Remove checkout").performClick()
        rule.waitUntil(5_000) { requests.isNotEmpty() }

        assertEquals("DELETE /api/v1/projects/p1/checkouts/$OrionNode", requests.single().first)
    }

    @Test
    fun removingTheLastCheckoutIsRefusedWithTheServersMessage() {
        removalAnswer = HttpStatusCode.Conflict to """{"error":"last_checkout","message":"This is the only checkout of backups. Remove the project instead."}"""
        showPage(store(TwoCheckouts))

        rule.onNodeWithContentDescription("More for the checkout on orion").performClick()
        rule.onNodeWithText("Remove checkout").performClick()
        rule.onNodeWithText("Remove checkout").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("This is the only checkout of backups. Remove the project instead.").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Remove the checkout on orion?").assertExists()
    }

    @Test
    fun anActiveProjectCanBeArchivedFromItsMenuButNotRemoved() {
        showPage(store(TwoCheckouts))

        rule.onNodeWithContentDescription("More for backups").performClick()
        rule.onNodeWithText("Remove").assertDoesNotExist()
        rule.onNodeWithText("Archive").performClick()
        rule.waitUntil(5_000) { requests.isNotEmpty() }

        assertEquals("PATCH /api/v1/projects/p1", requests.single().first)
        assertTrue(requests.single().second.contains("\"archived\":true"), requests.single().second)
    }

    @Test
    fun anArchivedProjectIsRestoredOrRemovedAfterConfirming() {
        showPage(store(TwoCheckouts.replace("\"created_at\":\"2026-10-22T10:00:00Z\"}", "\"archived_at\":\"2026-11-13T19:00:00Z\",\"created_at\":\"2026-10-22T10:00:00Z\"}")))

        rule.onNodeWithContentDescription("More for backups").performClick()
        rule.onNodeWithText("Restore").assertExists()
        rule.onNodeWithText("Remove").performClick()
        rule.onNodeWithText("Remove backups?").assertExists()
        assertTrue(requests.isEmpty())

        rule.onNodeWithText("Remove project").performClick()
        rule.waitUntil(5_000) { requests.isNotEmpty() }

        assertEquals("DELETE /api/v1/projects/p1", requests.single().first)
    }

    @Test
    fun addingACheckoutOffersCloneAndFolderOnlyForNodesWithoutOne() {
        showPage(store(TwoCheckouts))

        rule.onNodeWithText("Add a checkout").performClick()

        rule.onNodeWithText("Clone onto willow").assertExists()
        rule.onNodeWithText("Use a folder on willow").assertExists()
        rule.onAllNodesWithText("Clone onto orion").assertCountEquals(0)

        rule.onNodeWithText("Clone onto willow").performClick()
        rule.waitUntil(5_000) { requests.isNotEmpty() }

        assertEquals("POST /api/v1/projects/p1/checkout" to """{"node_id":"$WillowNode"}""", requests.single())
    }
}
