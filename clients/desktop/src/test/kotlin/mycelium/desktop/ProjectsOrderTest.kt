package mycelium.desktop

import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.hasClickAction
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.runBlocking
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.ProjectsScreen
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertTrue

private const val Laptop = "aaaaaaaa-0000-0000-0000-000000000001"
private const val Atlas = "aaaaaaaa-0000-0000-0000-000000000002"

private const val Nodes = """[
{"id":"$Laptop","name":"laptop","hostname":"laptop","os":"linux","arch":"x86_64","version":"0.1.0","online":true,"draining":false,"agents":[],"logins":[]},
{"id":"$Atlas","name":"atlas","hostname":"atlas","os":"linux","arch":"x86_64","version":"0.1.0","online":true,"draining":false,"agents":[],"logins":[]}]"""

private const val Forges = """[{"id":"f1","kind":"github","base_url":"https://github.com","account":"samexample","token_hint":"wxyz","organisations":["acme-org"],"created_at":"2026-10-22T10:00:00Z"}]"""

private const val Repositories = """{"repositories":[
{"forge_id":"f1","owner":"acme-org","name":"stalest","full_name":"acme-org/stalest","private":false,"updated_at":"2020-01-01T00:00:00Z"},
{"forge_id":"f1","owner":"acme-org","name":"freshest","full_name":"acme-org/freshest","private":false,"updated_at":"2026-10-20T00:00:00Z"},
{"forge_id":"f1","owner":"acme-org","name":"undated","full_name":"acme-org/undated","private":false}],
"organisations":[],"failures":[]}"""

private const val Projects = """[
{"id":"p1","node_id":"$Laptop","name":"stalest","path":"/p/stalest","forge_id":"f1","repository":"acme-org/stalest","created_at":"2026-10-22T10:00:00Z"},
{"id":"p2","node_id":"$Atlas","name":"homelab","path":"/p/homelab","forge_id":null,"repository":null,"created_at":"2026-10-21T10:00:00Z"}]"""

class ProjectsOrderTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private fun store(): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            when {
                path.endsWith("/repositories") -> respond(Repositories, HttpStatusCode.OK, json)
                path.endsWith("/projects") -> respond(Projects, HttpStatusCode.OK, json)
                path.endsWith("/forges") -> respond(Forges, HttpStatusCode.OK, json)
                path.endsWith("/nodes") -> respond(Nodes, HttpStatusCode.OK, json)
                path.endsWith("/conversations") -> respond("[]", HttpStatusCode.OK, json)
                else -> respond("{}", HttpStatusCode.OK, json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        runBlocking { store.refresh() }

        return store
    }

    private fun show() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); ProjectsScreen(store, state, onNewConversation = {}) } }
        rule.waitUntil(5_000) { rule.onAllNodesWithText("freshest").fetchSemanticsNodes().isNotEmpty() }
    }

    private fun top(text: String): Float = rule.onNodeWithText(text).fetchSemanticsNode().boundsInRoot.top

    private fun choose(filter: String, option: String) {
        rule.onNodeWithContentDescription(filter).performClick()
        rule.onNode(hasText(option) and hasClickAction()).performClick()
    }

    @Test
    fun setUpProjectsComeFirstThenTheOtherRepositoriesMostRecentlyUpdatedFirst() {
        show()

        assertTrue(top("stalest") < top("freshest"))
        assertTrue(top("freshest") < top("undated"))
    }

    @Test
    fun choosingANodeNarrowsTheRowsToWhatIsSetUpThere() {
        show()

        choose("Node: All nodes", "atlas")

        rule.onNodeWithText("homelab").assertExists()
        rule.onNodeWithText("stalest").assertDoesNotExist()
        rule.onNodeWithText("freshest").assertDoesNotExist()
    }

    @Test
    fun notSetUpAnywhereKeepsOnlyRepositoriesWithoutAProject() {
        show()

        choose("Node: All nodes", "Not set up anywhere")

        rule.onNodeWithText("freshest").assertExists()
        rule.onNodeWithText("undated").assertExists()
        rule.onNodeWithText("stalest").assertDoesNotExist()
        rule.onNodeWithText("homelab").assertDoesNotExist()
    }
}
