package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.test.requestFocus
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
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
import mycelium.ui.LocalPlatform
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Shell
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private class SidebarTogglePlatform : Platform {
    val preferences = mutableMapOf<String, String>()
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
    override fun loadPreference(key: String): String? = preferences[key]
    override fun savePreference(key: String, value: String) {
        preferences[key] = value
    }
}

private const val SidebarConversation = """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work","title":"Fix the build","status":"done",
"acp_session_id":null,"mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null}"""

class SidebarToggleTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val platform = SidebarTogglePlatform()

    private fun show(width: Dp) {
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val body = when {
                path.endsWith("/experiments") -> EnabledExperiments
                path.endsWith("/conversations") -> "[$SidebarConversation]"
                path.endsWith("/conversations/c1") -> SidebarConversation
                path.endsWith("/nodes") || path.endsWith("/groups") || path.endsWith("/events") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json"))
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }
        rule.setContent {
            CompositionLocalProvider(LocalPlatform provides platform) {
                MyceliumTheme(true) {
                    Box(Modifier.size(width, 900.dp)) {
                        Shell(store, platform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Chat("c1"))
                    }
                }
            }
        }
    }

    private fun count(description: String) = rule.onAllNodesWithContentDescription(description).fetchSemanticsNodes().size

    @Test
    fun hidingTheSidebarOnAWideWindowLeavesOneButtonThatDocksItAgain() {
        show(1600.dp)
        assertEquals(0, count("Show the sidebar"))

        rule.onNodeWithContentDescription("Hide the sidebar").performClick()
        assertEquals(0, count("Hide the sidebar"))
        assertEquals(1, count("Show the sidebar"))
        assertEquals("true", platform.preferences["sidebar-hidden"])

        rule.onNodeWithContentDescription("Show the sidebar").performClick()
        assertEquals(1, count("Hide the sidebar"))
        assertEquals(0, count("Show the sidebar"))
        assertEquals("false", platform.preferences["sidebar-hidden"])
    }

    @Test
    fun aSidePanelBesideTheChatCarriesNoShowTheSidebarButton() {
        show(1600.dp)
        rule.onNodeWithContentDescription("Hide the sidebar").performClick()
        rule.onNodeWithContentDescription("Open details").performClick()

        assertEquals(1, count("Show the sidebar"))
    }

    @Test
    fun aSidePanelThatTakesOverTheChatHasOneHeaderWithBackToConversation() {
        show(700.dp)
        rule.onNodeWithContentDescription("Open details").performClick()

        assertEquals(1, count("Back to conversation"))
        assertEquals(1, count("Show the sidebar"))
    }

    @Test
    fun controlBHidesAndShowsTheSidebar() {
        show(1600.dp)
        rule.onNodeWithContentDescription("Hide the sidebar").requestFocus()
        rule.onNodeWithContentDescription("Hide the sidebar").performKeyInput {
            keyDown(Key.CtrlLeft)
            pressKey(Key.B)
            keyUp(Key.CtrlLeft)
        }

        assertEquals(1, count("Show the sidebar"))

        rule.onNodeWithContentDescription("Show the sidebar").requestFocus()
        rule.onNodeWithContentDescription("Show the sidebar").performKeyInput {
            keyDown(Key.CtrlLeft)
            pressKey(Key.B)
            keyUp(Key.CtrlLeft)
        }

        assertEquals(1, count("Hide the sidebar"))
    }
}
