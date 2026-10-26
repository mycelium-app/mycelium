package mycelium.desktop

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.runBlocking
import mycelium.Connection
import mycelium.DeviceInfo
import mycelium.Platform
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Shell
import org.junit.Rule
import kotlin.test.Test

private object SidebarPlatform : Platform {
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

private fun conversation(id: String, node: String, title: String) = """{"id":"$id","owner":"u","node_id":"$node","agent":"claude","cwd":"/work/app","title":"$title","status":"review",
"acp_session_id":"s","mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null}"""

private fun node(id: String, name: String, online: Boolean, sessions: String) = """{"id":"$id","name":"$name","hostname":"$name","os":"linux","arch":"x86_64","version":"v0.1.13",
"online":$online,"draining":false,"agents":[],"logins":[],"sessions":$sessions}"""

private val ConversationsJson = "[" + listOf(
    conversation("c-live", "n-up", "Running here"),
    conversation("c-idle", "n-up", "Stopped here"),
    conversation("c-away", "n-down", "On a node that is away"),
).joinToString(",") + "]"

private val NodesJson = "[" + listOf(node("n-up", "laptop", true, """["c-live"]"""), node("n-down", "atlas", false, """["c-away"]""")).joinToString(",") + "]"

class MutedConversationsTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private fun showSidebar() {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val body = when {
                path.endsWith("/nodes") -> NodesJson
                path.endsWith("/conversations") -> ConversationsJson
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { Shell(store, SidebarPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Nodes) } }
    }

    @Test
    fun aConversationWhoseSessionIsRunningOnAnOnlineNodeIsNotMarkedNotRunning() {
        showSidebar()

        rule.onNodeWithContentDescription("Running here, Ready to review, Claude Code on laptop").assertExists()
    }

    @Test
    fun aConversationWhoseSessionIsNotRunningOnItsNodeIsMarkedNotRunning() {
        showSidebar()

        rule.onNodeWithContentDescription("Stopped here, Ready to review, Claude Code on laptop, not running").assertExists()
    }

    @Test
    fun aConversationOnAnOfflineNodeIsMarkedNotRunning() {
        showSidebar()

        rule.onNodeWithContentDescription("On a node that is away, Ready to review, Claude Code on atlas, not running").assertExists()
    }
}
