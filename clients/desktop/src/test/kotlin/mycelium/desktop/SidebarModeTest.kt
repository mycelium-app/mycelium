package mycelium.desktop

import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.runBlocking
import mycelium.Connection
import mycelium.DeviceInfo
import mycelium.Platform
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.Shell
import org.junit.Rule
import kotlin.test.Test

private object ModePlatform : Platform {
    override val originUrl: String? = null
    override val signsInThroughTheBrowser = false
    override fun loadConnection(): Connection? = null
    override fun saveConnection(connection: Connection?) = Unit
    override fun device() = DeviceInfo("Desktop", "host", "linux", "x86_64")
    override fun approveCode(): String? = null
    override fun currentPath() = "/"
    override fun navigate(url: String) = Unit
    override fun openUrl(url: String) = Unit
    override fun copyToClipboard(text: String) = Unit
}

class SidebarModeTest {
    @get:Rule
    val rule = createComposeRule()

    @Test
    fun projectsAndIssuesShowOnlyAlongsideWorkspaces() {
        val engine = MockEngine { request ->
            val body = if (request.url.encodedPath.endsWith("/nodes") || request.url.encodedPath.endsWith("/conversations")) "[]" else "{}"

            respond(body, HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json"))
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), CoroutineScope(Dispatchers.Unconfined))
        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { Shell(store, ModePlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}) } }

        rule.onNodeWithContentDescription("Projects").assertExists()
        rule.onNodeWithText("Chats").performClick()
        rule.onAllNodesWithContentDescription("Projects").assertCountEquals(0)
        rule.onAllNodesWithContentDescription("Issues").assertCountEquals(0)
        rule.onNodeWithText("Workspaces").performClick()
        rule.onNodeWithContentDescription("Issues").assertExists()
    }

    @Test
    fun switchingModesReopensWhatWasLastOpenInEach() {
        val conversation = """{"id":"w1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/app","title":"Fix the build","status":"done",
"acp_session_id":null,"mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null}"""
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val body = when {
                path.endsWith("/conversations") -> "[$conversation]"
                path.endsWith("/conversations/w1") -> conversation
                path.endsWith("/nodes") || path.endsWith("/events") || path.endsWith("/groups") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json"))
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), CoroutineScope(Dispatchers.Unconfined))
        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { Shell(store, ModePlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}) } }

        rule.onNodeWithText("Fix the build").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Fix the build").fetchSemanticsNodes().size == 2 }

        rule.onNodeWithText("Chats").performClick()
        rule.onAllNodesWithText("Fix the build").assertCountEquals(0)

        rule.onNodeWithText("Workspaces").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Fix the build").fetchSemanticsNodes().size == 2 }
    }
}
