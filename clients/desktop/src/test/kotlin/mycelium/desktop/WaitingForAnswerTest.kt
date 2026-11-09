package mycelium.desktop

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.getUnclippedBoundsInRoot
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.runBlocking
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.ConversationScreen
import mycelium.ui.MyTheme
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private fun conversationJson(status: String) = """{"id":"c1","owner":"u","node_id":"n","agent":"claude","cwd":"/work/mycelium","title":"Release","status":"$status",
"acp_session_id":"s1","mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null}"""

class WaitingForAnswerTest {
    @get:Rule
    val scopes = TestScopes()

    @get:Rule
    val compose = createComposeRule()

    private fun show(status: String) {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val conversation = conversationJson(status)
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            when {
                path.endsWith("/conversations") -> respond("[$conversation]", HttpStatusCode.OK, json)
                path.endsWith("/conversations/c1") -> respond(conversation, HttpStatusCode.OK, json)
                path.endsWith("/events") || path.endsWith("/queue") || path.endsWith("/nodes") || path.endsWith("/terminals") || path.endsWith("/watches") -> respond("[]", HttpStatusCode.OK, json)
                else -> respond("{}", HttpStatusCode.OK, json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        runBlocking { store.refresh(); store.open("c1") }
        compose.setContent {
            MyceliumTheme(true) {
                val state by store.state.collectAsState()

                Box(Modifier.fillMaxSize().background(MyTheme.colors.bgContent)) {
                    ConversationScreen(store, state, "c1", onArchived = {}, panels = false)
                }
            }
        }
        compose.waitForIdle()
    }

    @Test
    fun aConversationWaitingForInputSaysSoJustAboveTheComposer() {
        show("input")

        val line = compose.onNodeWithText("Waiting for your answer").getUnclippedBoundsInRoot()
        val composer = compose.onNode(hasSetTextAction()).getUnclippedBoundsInRoot()

        assertTrue(line.bottom <= composer.top)
        compose.captureScreen("3.8-waiting-for-an-answer")
    }

    @Test
    fun aConversationNotWaitingForInputDoesNotSayItIs() {
        show("done")

        assertEquals(0, compose.onAllNodesWithText("Waiting for your answer").fetchSemanticsNodes().size)
    }
}
