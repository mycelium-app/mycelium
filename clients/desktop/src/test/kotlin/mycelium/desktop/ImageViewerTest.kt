package mycelium.desktop

import androidx.compose.ui.input.key.Key
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.pressKey
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.runBlocking
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.state.sha256Hex
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Shell
import org.junit.Rule
import java.util.Base64
import kotlin.test.Test
import kotlin.test.assertEquals

private const val ChatJson = """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/app","title":"Fix the build","status":"done",
"acp_session_id":"s","mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null}"""

class ImageViewerTest {
    @get:Rule
    val rule = createComposeRule()

    private val first = testPng(tint = 0x2f9e44)
    private val second = testPng(tint = 0xc92a2a)

    private fun thread(): String {
        val sent = """{"conversation_id":"c1","seq":1,"kind":"user_message","payload":{"text":"look at these","attachments":[{"sha256":"${sha256Hex(first)}","name":"first.png","media_type":"image/png","size":${first.size}}]}}"""
        val data = Base64.getEncoder().encodeToString(second)
        val read = """{"conversation_id":"c1","seq":2,"kind":"acp_update","payload":{"update":{"sessionUpdate":"tool_call","toolCallId":"t1","title":"Read /work/second.png","kind":"read","status":"completed","content":[{"type":"content","content":{"type":"image","data":"$data","mimeType":"image/png"}}]}}}"""

        return "[$sent,$read]"
    }

    private fun open() {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            when {
                path.startsWith("/api/v1/attachments/") -> respond(first, HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "image/png"))
                path.endsWith("/events") -> respond(thread(), HttpStatusCode.OK, json)
                path.endsWith("/conversations") -> respond("[$ChatJson]", HttpStatusCode.OK, json)
                path.endsWith("/conversations/c1") -> respond(ChatJson, HttpStatusCode.OK, json)
                path.endsWith("/queue") || path.endsWith("/nodes") -> respond("[]", HttpStatusCode.OK, json)
                else -> respond("{}", HttpStatusCode.OK, json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), CoroutineScope(Dispatchers.Unconfined))
        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { Shell(store, QuietPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Chat("c1")) } }
    }

    @Test
    fun theViewerStepsToTheNextImageWithTheArrowKeyAndClosesWithEscape() {
        open()
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("first.png").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithContentDescription("first.png").performClick()
        rule.onNodeWithText("1 of 2").assertIsDisplayed()

        rule.onNodeWithContentDescription("Image: first.png").performKeyInput { pressKey(Key.DirectionRight) }
        rule.onNodeWithText("2 of 2").assertIsDisplayed()

        rule.onNodeWithContentDescription("Image: Read /work/second.png").performKeyInput { pressKey(Key.Escape) }
        rule.waitForIdle()
        assertEquals(0, rule.onAllNodesWithContentDescription("Close image").fetchSemanticsNodes().size)
    }

    @Test
    fun zoomInRaisesTheReadoutAndTheCloseButtonClosesTheViewer() {
        open()
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("first.png").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithContentDescription("first.png").performClick()
        rule.onNodeWithText("100%").assertIsDisplayed()
        rule.onNodeWithContentDescription("Zoom in").performClick()
        rule.onNodeWithText("125%").assertIsDisplayed()

        rule.onNodeWithContentDescription("Close image").performClick()
        rule.waitForIdle()
        assertEquals(0, rule.onAllNodesWithContentDescription("Close image").fetchSemanticsNodes().size)
    }
}
