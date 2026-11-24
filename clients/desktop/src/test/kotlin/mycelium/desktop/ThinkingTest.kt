package mycelium.desktop

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
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

private const val Working = """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/app","title":"Bump the UI kit","status":"working","acp_session_id":"s1","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","agent_active":true}"""

private const val Asked = """{"conversation_id":"c1","seq":1,"kind":"user_message","payload":{"text":"Bump the UI kit"},"created_at":"2026-10-22T10:00:00Z"}"""

private fun thought(seq: Int, text: String, at: String) =
    """{"conversation_id":"c1","seq":$seq,"kind":"acp_update","payload":{"update":{"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"$text"}}},"created_at":"$at"}"""

private fun reply(seq: Int, text: String, at: String) =
    """{"conversation_id":"c1","seq":$seq,"kind":"acp_update","payload":{"update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"$text"}}},"created_at":"$at"}"""

private fun runningTool(seq: Int, at: String) =
    """{"conversation_id":"c1","seq":$seq,"kind":"acp_update","payload":{"update":{"sessionUpdate":"tool_call","toolCallId":"t1","title":"pnpm -r test","kind":"execute","status":"in_progress"}},"created_at":"$at"}"""

class ThinkingTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private fun show(vararg events: String) {
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val body = when {
                path.endsWith("/conversations") -> "[$Working]"
                path.endsWith("/conversations/c1") -> Working
                path.endsWith("/events") -> events.joinToString(",", "[", "]")
                path.endsWith("/queue") || path.endsWith("/nodes") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json"))
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { Shell(store, QuietPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Chat("c1")) } }
    }

    @Test
    fun streamedThoughtsShowLiveInsideTheThinkingBlock() {
        show(Asked, thought(2, "The backups packages import the UI kit too.", "2026-10-22T10:00:01Z"))

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Thinking").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("The backups packages import the UI kit too.").assertExists()
        rule.onNodeWithContentDescription("Thinking", substring = true).assertExists()
    }

    @Test
    fun withoutThoughtContentTheBlockIsJustThinkingAndTheSeconds() {
        show(Asked)

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Thinking").fetchSemanticsNodes().isNotEmpty() }
        assertEquals(0, rule.onAllNodesWithText("Thought for", substring = true).fetchSemanticsNodes().size)
        assertEquals(0, rule.onAllNodesWithContentDescription("Show the agent's thinking").fetchSemanticsNodes().size)
    }

    @Test
    fun theNextEventCollapsesTheBlockToHowLongTheAgentThought() {
        show(Asked, thought(2, "Check the peer range.", "2026-10-22T10:00:01Z"), thought(3, " Then commit.", "2026-10-22T10:00:10Z"), reply(4, "Bumping it now.", "2026-10-22T10:00:11Z"))

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Thought for 9s").fetchSemanticsNodes().isNotEmpty() }
        assertEquals(0, rule.onAllNodesWithText("Check the peer range. Then commit.").fetchSemanticsNodes().size)
    }

    @Test
    fun aCollapsedThoughtExpandsAndCollapsesFromItsButton() {
        show(Asked, thought(2, "Check the peer range.", "2026-10-22T10:00:01Z"), reply(3, "Bumping it now.", "2026-10-22T10:00:04Z"))

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Show the agent's thinking").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Show the agent's thinking").performClick()
        rule.onNodeWithText("Check the peer range.").assertExists()

        rule.onNodeWithContentDescription("Hide the agent's thinking").performClick()
        assertEquals(0, rule.onAllNodesWithText("Check the peer range.").fetchSemanticsNodes().size)
    }

    @Test
    fun aRunningToolShowsNoThinkingBlock() {
        show(Asked, runningTool(2, "2026-10-22T10:00:01Z"))

        rule.waitUntil(5_000) { rule.onAllNodesWithText("pnpm -r test", substring = true).fetchSemanticsNodes().isNotEmpty() }
        assertEquals(0, rule.onAllNodesWithText("Thinking").fetchSemanticsNodes().size)
    }
}
