package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.unit.dp
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.runBlocking
import mycelium.state.Store
import mycelium.api.MyceliumApi
import mycelium.ui.LocalThreadSubagents
import mycelium.ui.MyceliumTheme
import mycelium.ui.ThreadSubagents
import mycelium.ui.ThreadView
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private const val Parent = """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/harbour","title":"Bump the UI kit","status":"working","acp_session_id":"s","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:05:00Z"}"""
private const val Waiting = """{"id":"c2","owner":"u","node_id":"n1","agent":"codex","cwd":"/work/harbour","title":"Run the e2e suite on the new kit","status":"input","parent_id":"c1","side_kind":"subagent","acp_session_id":"s","created_at":"2026-10-22T10:01:00Z","updated_at":"2026-10-22T10:05:00Z"}"""
private const val Working = """{"id":"c3","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/harbour","title":"Check the changelog wording","model":"Sonnet 5.5","status":"working","parent_id":"c1","side_kind":"subagent","acp_session_id":"s","created_at":"2026-10-22T10:02:00Z","updated_at":"2026-10-22T10:05:00Z"}"""
private const val WaitingEvents = """[{"conversation_id":"c2","seq":1,"kind":"acp_update","payload":{"sessionId":"s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"31 of 48 specs pass. Should I install Chromium so the whole suite can run?"}}}}]"""

class SubagentReplyTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val posts = java.util.concurrent.CopyOnWriteArrayList<Pair<String, String>>()

    private val opened = java.util.concurrent.CopyOnWriteArrayList<String>()

    private fun show() {
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val body = String(request.body.toByteArray())

            if (request.method == HttpMethod.Post) {
                posts += path to body
            }

            val response = when {
                path.endsWith("/conversations") -> "[$Parent,$Waiting,$Working]"
                path.endsWith("/conversations/c2/events") -> WaitingEvents
                path.endsWith("/events") || path.endsWith("/queue") || path.endsWith("/nodes") || path.endsWith("/groups") || path.endsWith("/workflows") -> "[]"
                else -> "{}"
            }

            respond(response, HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json"))
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        runBlocking { store.refresh() }
        rule.setContent {
            MyceliumTheme(true) {
                val state by store.state.collectAsState()

                CompositionLocalProvider(LocalThreadSubagents provides ThreadSubagents(store, state, "c1", { opened += it })) {
                    Box(Modifier.width(720.dp)) { ThreadView(emptyList(), onAnswer = { _, _ -> }, onStopWorkflow = {}) }
                }
            }
        }
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Reply to Run the e2e suite on the new kit").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun aSubagentWaitingForAnAnswerShowsItsQuestionTheWaitingLineAndAReplyFieldInItsCard() {
        show()

        rule.onNodeWithContentDescription("Run the e2e suite on the new kit, waiting for your answer").assertExists()
        rule.onNodeWithText("31 of 48 specs pass. Should I install Chromium so the whole suite can run?").assertExists()
        rule.onNodeWithText("Waiting for your answer").assertExists()
        rule.onNodeWithText("Reply to this subagent").assertExists()
        rule.onNodeWithContentDescription("Send to Run the e2e suite on the new kit").assertExists()
        rule.captureScreen("2.26-subagent-reply")
    }

    @Test
    fun aSubagentRowNamesItsHarnessModelAndStatusAndOpensTheSubagent() {
        show()

        rule.onNode(hasContentDescription("Check the changelog wording, Claude, Sonnet 5.5, Working", substring = true)).assertExists()
        rule.onNode(hasContentDescription("Run the e2e suite on the new kit, Codex, Needs input", substring = true) and hasClickAction()).performClick()

        assertEquals(listOf("c2"), opened.toList())
    }

    @Test
    fun sendPostsTheReplyUnchangedToThatSubagentsPromptRoute() {
        show()

        rule.onNodeWithContentDescription("Reply to Run the e2e suite on the new kit").performTextInput("Yes, install it")
        rule.onNodeWithContentDescription("Send to Run the e2e suite on the new kit").performClick()
        rule.waitUntil(5_000) { posts.any { it.first.endsWith("/conversations/c2/prompt") } }

        val body = posts.single { it.first.endsWith("/conversations/c2/prompt") }.second
        assertTrue(body.contains("\"text\":\"Yes, install it\""), body)
    }
}
