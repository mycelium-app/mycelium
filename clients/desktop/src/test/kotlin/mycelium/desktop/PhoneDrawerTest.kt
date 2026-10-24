package mycelium.desktop

import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipe
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
import mycelium.ui.LocalTouch
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Shell
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private object PhonePlatform : Platform {
    override val originUrl: String? = null
    override val signsInThroughTheBrowser = false
    override fun loadConnection(): Connection? = null
    override fun saveConnection(connection: Connection?) = Unit
    override fun device() = DeviceInfo("Phone", "host", "android", "arm64")
    override fun approveCode(): String? = null
    override fun currentPath() = "/"
    override fun navigate(url: String) = Unit
    override fun openUrl(url: String) = Unit
    override fun copyToClipboard(text: String) = Unit
}

private const val Conversation = """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work","title":"Fix the build","status":"done",
"acp_session_id":null,"mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null}"""

class PhoneDrawerTest {
    @get:Rule
    val rule = createComposeRule()

    private fun show() {
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val body = when {
                path.endsWith("/conversations") -> "[$Conversation]"
                path.endsWith("/conversations/c1") -> Conversation
                path.endsWith("/nodes") || path.endsWith("/groups") || path.endsWith("/events") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json"))
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), CoroutineScope(Dispatchers.Unconfined))
        runBlocking { store.refresh() }
        rule.setContent {
            MyceliumTheme(true) {
                CompositionLocalProvider(LocalTouch provides true) {
                    Shell(store, PhonePlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Chat("c1"))
                }
            }
        }
    }

    private fun drawerIsOpen() = rule.onAllNodesWithText("Schedules").fetchSemanticsNodes().isNotEmpty()

    @Test
    fun swipingRightFromTheLeftEdgeOpensTheSidebar() {
        show()

        rule.onRoot().performTouchInput { swipe(Offset(4f, height / 2f), Offset(width * 0.8f, height / 2f)) }
        rule.waitForIdle()

        assertEquals(true, drawerIsOpen())
    }

    @Test
    fun swipingRightFromTheMiddleOfTheScreenOpensTheSidebar() {
        show()

        rule.onRoot().performTouchInput { swipe(Offset(width / 2f, height / 2f), Offset(width * 0.95f, height / 2f)) }
        rule.waitForIdle()

        assertEquals(true, drawerIsOpen())
    }

    @Test
    fun swipingLeftOnTheConversationLeavesTheSidebarClosed() {
        show()

        rule.onRoot().performTouchInput { swipe(Offset(width * 0.8f, height / 2f), Offset(width * 0.2f, height / 2f)) }
        rule.waitForIdle()

        assertEquals(false, drawerIsOpen())
    }

    @Test
    fun swipingLeftClosesTheOpenSidebar() {
        show()
        rule.onNodeWithContentDescription("Conversations").performClick()
        rule.waitForIdle()
        assertEquals(true, drawerIsOpen())

        rule.onRoot().performTouchInput { swipe(Offset(width * 0.7f, height / 2f), Offset(width * 0.1f, height / 2f)) }

        rule.waitUntil(5_000) { !drawerIsOpen() }
        rule.waitForIdle()
    }

    @Test
    fun theSidePanelSheetHasNoConversationsMenuButtonOfItsOwn() {
        show()
        rule.onNodeWithContentDescription("Git").performClick()
        rule.waitForIdle()

        assertEquals(1, rule.onAllNodesWithContentDescription("Conversations").fetchSemanticsNodes().size)
    }
}
