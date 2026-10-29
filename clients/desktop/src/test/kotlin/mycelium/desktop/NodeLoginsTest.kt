package mycelium.desktop

import androidx.compose.ui.graphics.toAwtImage
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.isRoot
import java.io.File
import javax.imageio.ImageIO
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import java.util.concurrent.CopyOnWriteArrayList
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

private const val LoginsJson = """[{"id":"n1","name":"laptop","hostname":"laptop","os":"linux","arch":"x86_64","version":"v0.16.1",
"online":true,"draining":false,"agents":[{"agent":"claude","version":"0.94.0","available":true}],"logins":[
{"agent":"claude","account":"sam@example.com","method":"oauth_token","state":"signed_in"},
{"agent":"claude","account":"account-1970","method":"oauth_token","state":"signed_out","managed":true}]}]"""

private object LoginsPlatform : Platform {
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

class NodeLoginsTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val deletes = CopyOnWriteArrayList<String>()

    private fun showNodes() {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            if (request.method == HttpMethod.Delete) {
                deletes += path
            }

            val body = when {
                path.endsWith("/nodes") -> LoginsJson
                path.endsWith("/conversations") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { Shell(store, LoginsPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Nodes) } }
    }

    private fun shot(name: String) {
        val roots = rule.onAllNodes(isRoot()).fetchSemanticsNodes().indices

        for (index in roots) {
            val out = File("build/screens/$name-$index.png")
            out.parentFile.mkdirs()
            ImageIO.write(rule.onAllNodes(isRoot())[index].captureToImage().toAwtImage(), "png", out)
        }
    }

    @Test
    fun onlyALoginMyceliumMadeOffersAMenu() {
        showNodes()

        rule.onNodeWithContentDescription("More for Claude Code account-1970").assertExists()
        assertEquals(0, rule.onAllNodesWithContentDescription("More for Claude Code sam@example.com").fetchSemanticsNodes().size)
    }

    @Test
    fun removingALoginAsksFirstThenTellsTheServer() {
        showNodes()

        rule.onNodeWithContentDescription("More for Claude Code account-1970").performClick()
        shot("node-login-menu")
        rule.onNodeWithText("Remove login").performClick()
        rule.onNodeWithText("Remove account-1970 from laptop?").assertExists()
        shot("node-login-remove")
        assertEquals(emptyList(), deletes.toList())

        rule.onNodeWithText("Remove login").performClick()
        rule.waitUntil(5_000) { deletes.isNotEmpty() }

        assertEquals(listOf("/api/v1/nodes/n1/logins/claude/account-1970"), deletes.toList())
    }
}
