package mycelium.desktop

import androidx.compose.ui.test.ComposeUiTest
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performMouseInput
import androidx.compose.ui.test.rightClick
import androidx.compose.ui.test.runDesktopComposeUiTest
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpStatusCode
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
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

private const val PopOutConversations = """[
{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/harbour","title":"Bump the UI kit to 0.31.2","status":"working","acp_session_id":null,"mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:05:00Z","archived_at":null},
{"id":"c2","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/ledger","title":"Uploads survive a 503 retry","status":"working","acp_session_id":null,"mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:04:00Z","archived_at":null}]"""

private class WindowsPlatform(override val windowChat: String? = null) : Platform {
    val open = MutableStateFlow<Set<String>>(emptySet())
    val focused = mutableListOf<String>()

    override val originUrl: String? = null
    override val signsInThroughTheBrowser = false
    override val opensChatWindows = true
    override val chatWindows: StateFlow<Set<String>> = open
    override fun openChatWindow(id: String, atPointer: Boolean) {
        open.value += id
    }
    override fun focusChatWindow(id: String) {
        focused += id
    }
    override fun closeChatWindow(id: String) {
        open.value -= id
    }
    override fun loadConnection(): Connection? = null
    override fun saveConnection(connection: Connection?) = Unit
    override fun device() = DeviceInfo("Desktop", "host", "linux", "x86_64")
    override fun approveCode(): String? = null
    override fun currentPath() = "/"
    override fun navigate(url: String) = Unit
    override fun openUrl(url: String) = Unit
    override fun copyToClipboard(text: String) = Unit
}

@OptIn(ExperimentalTestApi::class)
class PopOutTest {
    @get:Rule
    val scopes = TestScopes()

    private fun ComposeUiTest.show(platform: WindowsPlatform, route: Route = Route.Chat("c1")) {
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val body = when {
                path.endsWith("/conversations") -> PopOutConversations
                path.endsWith("/nodes") || path.endsWith("/events") || path.endsWith("/assistants") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, JsonHeaders)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        runBlocking { store.refresh() }
        setContent { MyceliumTheme(true) { Shell(store, platform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = route) } }
        waitUntil(timeoutMillis = 5_000) { onAllNodesWithText("Uploads survive a 503 retry").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun theConversationMenuOpensTheChatInItsOwnWindowAndTheMainWindowSaysWhereItWent() = runDesktopComposeUiTest(width = 1440, height = 900) {
        val platform = WindowsPlatform()
        show(platform)

        onNodeWithContentDescription("More").performClick()
        onNodeWithText("Open in new window").performClick()

        assertEquals(setOf("c1"), platform.open.value)
        onNodeWithText("This chat is open in its own window.").assertExists()
        onNode(hasContentDescription(", open in its own window", substring = true)).assertExists()

        onNodeWithText("Go to window").performClick()
        assertEquals(listOf("c1"), platform.focused)
    }

    @Test
    fun showHereClosesTheChatWindowAndShowsTheChatInTheMainWindowAgain() = runDesktopComposeUiTest(width = 1440, height = 900) {
        val platform = WindowsPlatform()
        platform.open.value = setOf("c1")
        show(platform)

        onNodeWithText("Show here").performClick()

        assertEquals(emptySet(), platform.open.value)
        onAllNodesWithText("This chat is open in its own window.").assertCountEquals(0)
    }

    @Test
    fun theSidebarRowMenuOpensTheChatInItsOwnWindow() = runDesktopComposeUiTest(width = 1440, height = 900) {
        val platform = WindowsPlatform()
        show(platform, Route.Inbox)

        onAllNodesWithText("Uploads survive a 503 retry").onFirst().performMouseInput { rightClick() }
        onNodeWithText("Open in new window").performClick()

        assertEquals(setOf("c2"), platform.open.value)
    }

    @Test
    fun closingAChatWindowShowsThatChatInTheMainWindow() = runDesktopComposeUiTest(width = 1440, height = 900) {
        val platform = WindowsPlatform()
        platform.open.value = setOf("c2")
        show(platform, Route.Inbox)

        platform.closeChatWindow("c2")

        waitUntil(timeoutMillis = 5_000) { onAllNodesWithText("Uploads survive a 503 retry").fetchSemanticsNodes().size > 1 }
    }

    @Test
    fun aChatWindowShowsOnlyItsConversationAndOffersToShowItInTheMainWindow() = runDesktopComposeUiTest(width = 1440, height = 900) {
        val platform = WindowsPlatform(windowChat = "c1")
        platform.open.value = setOf("c1")
        val engine = MockEngine { request ->
            val body = when {
                request.url.encodedPath.endsWith("/conversations") -> PopOutConversations
                else -> "[]"
            }

            respond(body, HttpStatusCode.OK, JsonHeaders)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        runBlocking { store.refresh() }
        setContent { MyceliumTheme(true) { Shell(store, platform, Me("u", "sam@example.com", "Sam"), onSignOut = {}) } }
        waitUntil(timeoutMillis = 5_000) { onAllNodesWithText("Bump the UI kit to 0.31.2").fetchSemanticsNodes().isNotEmpty() }

        onAllNodesWithText("Uploads survive a 503 retry").assertCountEquals(0)
        onAllNodesWithText("New conversation").assertCountEquals(0)

        onNodeWithContentDescription("More").performClick()
        onNodeWithText("Show in main window").performClick()

        assertEquals(emptySet(), platform.open.value)
    }
}
