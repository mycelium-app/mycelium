package mycelium.desktop

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
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
import mycelium.ui.Route
import mycelium.ui.Shell
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private const val BehindJson = """[{"id":"n1","name":"laptop","hostname":"laptop","os":"linux","arch":"x86_64","version":"v0.1.13","update_available":"v0.1.16",
"online":true,"draining":false,"agents":[
{"agent":"claude","version":"@agentclientprotocol/claude-agent-acp 0.94.0","available":true,"update_available":"0.95.1"},
{"agent":"codex","version":"@agentclientprotocol/codex-acp 2.2.2","available":true}],"logins":[]}]"""

private object NodesPagePlatform : Platform {
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

class NodeVersionsTest {
    @get:Rule
    val rule = createComposeRule()

    private fun showNodes(nodes: String) {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val body = when {
                path.endsWith("/nodes") -> nodes
                path.endsWith("/conversations") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), CoroutineScope(Dispatchers.Unconfined))
        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { Shell(store, NodesPagePlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Nodes) } }
    }

    @Test
    fun aNodeAndAnAgentBehindTheLatestReleaseShowTheNewerVersionNextToTheirOwn() {
        showNodes(BehindJson)

        rule.onNodeWithText("v0.1.13 · v0.1.16 available", substring = true).assertExists()
        rule.onNodeWithText("0.94.0 · 0.95.1 available").assertExists()
    }

    @Test
    fun anAgentThatIsUpToDateShowsOnlyItsOwnVersion() {
        showNodes(BehindJson)

        rule.onNodeWithText("2.2.2").assertExists()
        assertEquals(0, rule.onAllNodesWithText("2.2.2 ·", substring = true).fetchSemanticsNodes().size)
    }
}
