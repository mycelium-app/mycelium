package mycelium.desktop

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
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
import kotlin.test.assertEquals

private const val AtlasJson = """[{"id":"n1","name":"atlas","hostname":"atlas","os":"linux","arch":"x86_64","version":"v0.34.56","address":"192.0.2.10","online":true,"draining":false,"agents":[],"logins":[]}]"""

private object RedirectPlatform : Platform {
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

class ProjectsAndIssuesRedirectTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private fun showShell(route: Route): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val body = when {
                path.endsWith("/nodes") -> AtlasJson
                path.endsWith("/conversations") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { Shell(store, RedirectPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = route) } }
        rule.waitForIdle()

        return store
    }

    private fun opensHistory(address: String) {
        val store = showShell(Route.Inbox)

        rule.runOnIdle { store.openScreen(address) }
        rule.waitForIdle()

        rule.onNodeWithText("Past conversations").assertExists()
    }

    @Test
    fun theOldProjectsAddressOpensHistory() {
        opensHistory("projects")
    }

    @Test
    fun theOldIssuesAddressOpensHistory() {
        opensHistory("issues")
    }

    @Test
    fun theSidebarHasNoProjectsOrIssuesButtons() {
        showShell(Route.New)

        for (name in listOf("Projects", "Issues")) {
            assertEquals(0, rule.onAllNodesWithContentDescription(name).fetchSemanticsNodes().size, name)
            assertEquals(0, rule.onAllNodesWithText(name).fetchSemanticsNodes().size, name)
        }
    }
}
