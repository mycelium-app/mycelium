package mycelium.desktop

import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
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
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Shell
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private fun conversation(status: String, agentActive: Boolean) =
    """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/app","title":"Plan the rollout","status":"$status","acp_session_id":"s1","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","agent_active":$agentActive}"""

private const val EndedTurn = """[{"conversation_id":"c1","seq":1,"kind":"user_message","payload":{"text":"go"}},{"conversation_id":"c1","seq":2,"kind":"turn_end","payload":{"stopReason":"end_turn"}}]"""

class ComposerWorkingTest {
    @get:Rule
    val rule = createComposeRule()

    private fun show(conversation: String) {
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val body = when {
                path.endsWith("/conversations") -> "[$conversation]"
                path.endsWith("/conversations/c1") -> conversation
                path.endsWith("/events") -> EndedTurn
                path.endsWith("/queue") || path.endsWith("/nodes") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json"))
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), CoroutineScope(Dispatchers.Unconfined))
        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { Shell(store, QuietPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Chat("c1")) } }
    }

    @Test
    fun theComposerOffersStopWhileTheAgentStreamsAnUnpromptedReplyInAReviewConversation() {
        show(conversation("review", agentActive = true))

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Stop the agent").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun theComposerDoesNotOfferStopWhileOnlyBackgroundAgentsRun() {
        show(conversation("working", agentActive = false))

        rule.waitUntil(5_000) { rule.onAllNodes(hasSetTextAction()).fetchSemanticsNodes().isNotEmpty() }
        assertEquals(0, rule.onAllNodesWithContentDescription("Stop the agent").fetchSemanticsNodes().size)
    }
}
