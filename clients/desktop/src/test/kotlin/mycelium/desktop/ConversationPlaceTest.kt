package mycelium.desktop

import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
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
import mycelium.Platform
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.ConversationScreen
import mycelium.ui.LocalPlatform
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private class OpeningPlatform : Platform {
    val opened = mutableListOf<String>()

    override val originUrl: String? = null
    override val signsInThroughTheBrowser = false
    override fun loadConnection(): mycelium.Connection? = null
    override fun saveConnection(connection: mycelium.Connection?) = Unit
    override fun device() = mycelium.DeviceInfo("Desktop", "host", "linux", "x86_64")
    override fun approveCode(): String? = null
    override fun currentPath() = "/"
    override fun navigate(url: String) = Unit
    override fun openUrl(url: String) {
        opened += url
    }
    override fun copyToClipboard(text: String) = Unit
}

private fun placed(cwd: String, extra: String) =
    """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"$cwd","title":"Fix login","status":"review","acp_session_id":"s","mode":null,"model":null,"project_id":"p1",$extra
"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null}"""

private const val PlaceProjects = """[{"id":"p1","node_id":"n1","name":"app","path":"/work/app","created_at":"2026-10-22T10:00:00Z"}]"""

class ConversationPlaceTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val platform = OpeningPlatform()

    private fun show(conversation: String) {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val body = when {
                path.endsWith("/experiments") -> EnabledExperiments
                path.endsWith("/conversations") -> "[$conversation]"
                path.endsWith("/conversations/c1") -> conversation
                path.endsWith("/projects") -> PlaceProjects
                path.endsWith("/nodes") || path.endsWith("/events") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }
        runBlocking { store.refreshProjects() }
        rule.setContent { MyceliumTheme(true) { CompositionLocalProvider(LocalPlatform provides platform) { val state by store.state.collectAsState(); ConversationScreen(store, state, "c1", onArchived = {}) } } }
    }

    @Test
    fun the_composer_footer_shows_worktree_branch_and_pull_request() {
        show(placed("/work/app-worktrees/feature-login", """"branch":"feature/login","pull_request_label":"PR #12","pull_request_url":"https://forge.example/pull/12","""))

        rule.onNodeWithText("Worktree").assertExists()
        rule.onNodeWithText("feature/login").assertExists()
        rule.onNodeWithContentDescription("Open PR #12").performClick()
        assertEquals(listOf("https://forge.example/pull/12"), platform.opened)
    }

    @Test
    fun a_conversation_in_a_projects_own_folder_is_local_and_one_with_nothing_known_shows_no_footer() {
        show(placed("/work/app", ""))

        rule.onNodeWithText("Local").assertExists()
        assertEquals(0, rule.onAllNodesWithText("Worktree").fetchSemanticsNodes().size)
    }
}
