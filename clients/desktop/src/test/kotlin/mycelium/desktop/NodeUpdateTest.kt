package mycelium.desktop

import androidx.compose.ui.test.junit4.createComposeRule
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

private fun nodeJson(version: String, message: String? = null) = """[{"id":"n1","name":"willow","hostname":"willow","os":"linux","arch":"x86_64","version":"$version",
"online":true,"draining":false${message?.let { ""","update_message":"$it"""" } ?: ""},"agents":[],"logins":[]}]"""

private object UpdatePlatform : Platform {
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

class NodeUpdateTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val updates = CopyOnWriteArrayList<String>()

    private fun openNode(nodes: String) {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        updates.clear()
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            if (request.method == HttpMethod.Post && path.endsWith("/update")) {
                updates += path

                return@MockEngine respond("", HttpStatusCode.NoContent)
            }

            val body = when {
                path.endsWith("/version") -> """{"version":"v0.3.6"}"""
                path.endsWith("/nodes") -> nodes
                path.endsWith("/conversations") -> "[]"
                request.method == HttpMethod.Post && path.endsWith("/watch") -> """{"history":[]}"""
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { Shell(store, UpdatePlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Nodes) } }
        rule.onNodeWithContentDescription("Open willow").performClick()
        rule.waitForIdle()
    }

    @Test
    fun aNodeOlderThanTheControlPlaneOffersAnUpdateThatPostsToTheNode() {
        openNode(nodeJson("v0.3.1"))

        rule.onNodeWithText("Update to v0.3.6").performClick()

        rule.waitUntil(5_000) { updates.isNotEmpty() }
        assertEquals(listOf("/api/v1/nodes/n1/update"), updates)
    }

    @Test
    fun aNodeOnTheControlPlanesVersionOffersNoUpdate() {
        openNode(nodeJson("v0.3.6"))

        rule.onNodeWithText("Update to v0.3.6").assertDoesNotExist()
    }

    @Test
    fun theLastUpdateMessageIsShownOnTheNodePage() {
        openNode(nodeJson("v0.3.6", "This node was installed by Nix; update it through the flake"))

        rule.onNodeWithText("This node was installed by Nix; update it through the flake").assertExists()
    }
}
