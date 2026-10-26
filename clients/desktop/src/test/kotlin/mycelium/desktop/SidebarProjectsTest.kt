package mycelium.desktop

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasText
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.runBlocking
import mycelium.Platform
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Shell
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private class RememberingPlatform : Platform {
    private val preferences = mutableMapOf<String, String>()

    override val originUrl: String? = null
    override val signsInThroughTheBrowser = false
    override fun loadConnection(): mycelium.Connection? = null
    override fun saveConnection(connection: mycelium.Connection?) = Unit
    override fun device() = mycelium.DeviceInfo("Desktop", "host", "linux", "x86_64")
    override fun approveCode(): String? = null
    override fun currentPath() = "/"
    override fun navigate(url: String) = Unit
    override fun openUrl(url: String) = Unit
    override fun copyToClipboard(text: String) = Unit
    override fun loadPreference(key: String): String? = preferences[key]
    override fun savePreference(key: String, value: String) {
        preferences[key] = value
    }
}

private fun row(id: String, title: String, project: String?, status: String = "working", extra: String = "") =
    """{"id":"$id","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/$id","title":"$title","status":"$status","acp_session_id":"s","mode":null,"model":null,"project_id":${project?.let { "\"$it\"" }},$extra
"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null,"pinned_at":null}"""

private const val SidebarNodes = """[{"id":"n1","name":"laptop","hostname":"laptop","os":"linux","arch":"x86_64","version":"0.10.0","online":true,"draining":false,"agents":[],"logins":[]}]"""

private const val SidebarProjects = """[
{"id":"p1","node_id":"n1","name":"alpha","path":"/work/alpha","created_at":"2026-10-22T10:00:00Z"},
{"id":"p2","node_id":"n1","name":"beta","path":"/work/beta","created_at":"2026-10-22T10:00:00Z"}]"""

class SidebarProjectsTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private fun show(conversations: String) {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val body = when {
                path.endsWith("/experiments") -> EnabledExperiments
                path.endsWith("/conversations") -> "[$conversations]"
                path.endsWith("/nodes") -> SidebarNodes
                path.endsWith("/projects") -> SidebarProjects
                path.endsWith("/groups") || path.endsWith("/worktrees") || path.endsWith("/events") || path.endsWith("/queue") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { Shell(store, RememberingPlatform(), Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.New) } }
    }

    private fun shown(text: String): Int {
        rule.waitForIdle()

        return rule.onAllNodesWithText(text, substring = true).fetchSemanticsNodes().size
    }

    @Test
    fun the_project_filter_narrows_the_conversation_list() {
        show("${row("a1", "Fix login", "p1")},${row("b1", "Tidy docs", "p2")},${row("c1", "Loose end", null)}")
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Fix login", substring = true).fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithContentDescription("Project: All projects").performClick()
        rule.onNode(hasText("beta") and hasClickAction()).performClick()

        assertEquals(1, shown("Tidy docs"))
        assertEquals(0, shown("Fix login"))
        assertEquals(0, shown("Loose end"))
    }

    @Test
    fun settled_is_folded_until_opened() {
        show("${row("d1", "Shipped fix", "p1", status = "done")},${row("w1", "Busy fix", "p1")}")
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Busy fix", substring = true).fetchSemanticsNodes().isNotEmpty() }

        assertEquals(0, shown("Shipped fix"))

        rule.onNodeWithContentDescription("Settled, 1 conversations, collapsed").performClick()

        assertEquals(1, shown("Shipped fix"))
        rule.onNodeWithContentDescription("Settled, 1 conversations, expanded").performClick()
        assertEquals(0, shown("Shipped fix"))
    }

    @Test
    fun a_row_shows_its_branch_and_pull_request() {
        show(row("a1", "Fix login", "p1", extra = """"branch":"feature/login","pull_request_label":"PR #12","pull_request_url":"https://forge.example/pull/12","""))
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Fix login", substring = true).fetchSemanticsNodes().isNotEmpty() }

        assertEquals(1, shown("feature/login"))
        assertEquals(1, shown("PR #12"))
    }
}
