package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.unit.dp
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import java.util.concurrent.atomic.AtomicInteger
import kotlin.test.Test
import kotlin.test.assertEquals
import mycelium.Connection
import mycelium.DeviceInfo
import mycelium.Platform
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.NodesScreen
import mycelium.ui.SessionsSettings
import org.junit.Rule

private object ListPlatform : Platform {
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

private const val OneNode = """[{"id":"n1","name":"willow","hostname":"willow","os":"linux","arch":"x86_64","version":"0.1.0","online":true,"draining":false,"agents":[],"logins":[]}]"""

private const val TwoSessions = """[{"id":"s1","kind":"browser","label":"Firefox","hostname":"laptop","last_used_at":"2026-10-22T10:00:00Z"},{"id":"s2","kind":"device","label":"Pixel","hostname":"phone","last_used_at":"2026-10-22T10:00:00Z"}]"""

class ListLoadStatesTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val nodeRequests = AtomicInteger()

    private fun store(): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            when {
                path.endsWith("/nodes") && nodeRequests.incrementAndGet() == 1 -> respond("{}", HttpStatusCode.InternalServerError, json)
                path.endsWith("/nodes") -> respond(OneNode, HttpStatusCode.OK, json)
                path.endsWith("/sessions") -> respond(TwoSessions, HttpStatusCode.OK, json)
                else -> respond("[]", HttpStatusCode.OK, json)
            }
        }

        return Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
    }

    @Test
    fun aNodesListThatFailsToLoadSaysSoAndRetryLoadsIt() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); NodesScreen(store, state, ListPlatform) } }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Could not load nodes", substring = true).fetchSemanticsNodes().isNotEmpty() }
        assertEquals(0, rule.onAllNodesWithText("No nodes", substring = true).fetchSemanticsNodes().size)

        rule.onNodeWithContentDescription("Retry loading nodes").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("willow").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun searchingSessionsKeepsTheMatchingRows() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { Box(Modifier.width(900.dp)) { SessionsSettings(store, onSignOut = {}) } } }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Firefox on laptop").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Search sessions").performTextInput("pixel")
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Firefox on laptop").fetchSemanticsNodes().isEmpty() }
        rule.onNodeWithText("Pixel on phone").assertExists()
    }
}
