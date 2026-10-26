package mycelium.desktop

import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.test.junit4.createComposeRule
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
import mycelium.api.Conversation
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.ConversationScreen
import mycelium.ui.MyceliumTheme
import mycelium.ui.NewConversationScreen
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private const val NodeId = "aaaaaaaa-0000-0000-0000-000000000001"

private const val NodesJson = """[{"id":"$NodeId","name":"laptop","hostname":"laptop","os":"linux","arch":"x86_64","version":"0.10.0","online":true,"draining":false,
"agents":[{"agent":"claude","version":"2.1.308","available":true}],"logins":[]}]"""

private const val ProjectsJson = """[{"id":"p1","node_id":"$NodeId","name":"acme-backups","path":"/p/acme-backups","created_at":"2026-10-22T10:00:00Z"}]"""

private const val UnboundConversation = """{"id":"c1","owner":"u","node_id":"$NodeId","agent":"claude","cwd":"/home/claude/app","title":"Imported","status":"done",
"acp_session_id":null,"mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null}"""

private val BoundConversation = UnboundConversation.replace("\"created_at\"", "\"project_id\":\"p1\",\"created_at\"")

class ProjectBreadcrumbTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val requests = java.util.Collections.synchronizedList(mutableListOf<Triple<HttpMethod, String, String>>())

    private fun store(): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            if (request.method != HttpMethod.Get) {
                requests += Triple(request.method, path, String(request.body.toByteArray()))
            }

            val body = when {
                path.endsWith("/experiments") -> EnabledExperiments
                request.method == HttpMethod.Patch -> BoundConversation
                request.method == HttpMethod.Post && path.endsWith("/projects") -> ProjectsJson.removePrefix("[").removeSuffix("]")
                request.method == HttpMethod.Post && path.endsWith("/conversations") -> UnboundConversation
                path.endsWith("/projects") -> ProjectsJson
                path.endsWith("/nodes") -> NodesJson
                path.endsWith("/conversations") -> "[$UnboundConversation]"
                path.endsWith("/conversations/c1") -> UnboundConversation
                path.endsWith("/events") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        runBlocking { store.refresh() }
        runBlocking { store.refreshProjects() }

        return store
    }

    @Test
    fun a_new_conversation_starts_on_no_project_and_the_breadcrumb_picks_one() {
        val store = store()
        var created: Conversation? = null
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); NewConversationScreen(store, state, onCreated = { created = it }) } }

        rule.onNodeWithContentDescription("Project: No project").performClick()
        rule.onNodeWithText("acme-backups").performClick()
        rule.onNodeWithContentDescription("Project: acme-backups").assertExists()
        rule.onNodeWithContentDescription("Send").performClick()

        rule.waitUntil(5_000) { created != null }
        assertEquals("""{"node_id":"$NodeId","agent":"claude","project_id":"p1"}""", requests.single().third)
    }

    @Test
    fun the_conversation_breadcrumb_moves_it_into_a_project() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); ConversationScreen(store, state, "c1", onArchived = {}) } }

        rule.onNodeWithContentDescription("Project: No project").performClick()
        rule.onNodeWithText("acme-backups").performClick()

        rule.waitUntil(5_000) { requests.isNotEmpty() }
        assertEquals("""{"project_id":"p1"}""", requests.single().third)
        rule.onNodeWithContentDescription("Project: acme-backups").assertExists()
    }

    @Test
    fun an_unbound_conversation_offers_to_set_up_its_folder_as_a_project() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); ConversationScreen(store, state, "c1", onArchived = {}) } }

        rule.onNodeWithContentDescription("Project: No project").performClick()
        rule.onNodeWithText("Set up app as a project").performClick()

        rule.waitUntil(5_000) { requests.isNotEmpty() }
        assertEquals("""{"node_id":"$NodeId","path":"/home/claude/app"}""", requests.single().third)
    }
}
