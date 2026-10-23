package mycelium.desktop

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
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

private class RowsPlatform : Platform {
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
    override fun loadPreference(key: String): String? = null
    override fun savePreference(key: String, value: String) = Unit
}

private fun conversation(id: String, title: String, status: String) = """{"id":"$id","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/$id","title":"$title","status":"$status","project_id":"p1",
"acp_session_id":null,"mode":null,"model":null,"group_id":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null,"pinned_at":null}"""

private const val ProjectsJson = """[{"id":"p1","node_id":"n1","name":"Pipeline","path":"/work/pipeline","icon":"folder","created_at":"2026-10-22T10:00:00Z"}]"""

private const val NodesJson = """[{"id":"n1","name":"Workbench","hostname":"bench","os":"linux","arch":"x86_64","version":"1.0.0","online":true,"draining":false,"sessions":["w1","w2","w3"]}]"""

class ConversationRowStatusTest {
    @get:Rule
    val rule = createComposeRule()

    @Test
    fun eachRowNamesItsStateAgentAndNodeInWords() {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val rows = listOf(conversation("w1", "Build the thing", "working"), conversation("w2", "Pick a colour", "input"), conversation("w3", "Break the build", "failed"))
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val body = when {
                path.endsWith("/conversations") -> "[${rows.joinToString(",")}]"
                path.endsWith("/projects") -> ProjectsJson
                path.endsWith("/nodes") -> NodesJson
                path.endsWith("/groups") || path.endsWith("/events") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), CoroutineScope(Dispatchers.Unconfined))
        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { Shell(store, RowsPlatform(), Me("u", "sam@example.com", "Sam"), onSignOut = {}) } }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Build the thing").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithContentDescription("Build the thing, Working, Claude Code on Workbench").assertExists()
        rule.onNodeWithContentDescription("Pick a colour, Needs input, Claude Code on Workbench").assertExists()
        rule.onNodeWithContentDescription("Break the build, Failed, Claude Code on Workbench").assertExists()
    }
}
