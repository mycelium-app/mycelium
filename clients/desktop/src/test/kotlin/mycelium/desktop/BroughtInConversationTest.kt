package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
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
import mycelium.api.Event
import mycelium.api.MyceliumApi
import mycelium.api.MyceliumJson
import mycelium.api.StreamMessage
import mycelium.state.Store
import mycelium.state.ThreadItem
import mycelium.ui.BroughtInActions
import mycelium.ui.LocalBroughtIn
import mycelium.ui.MyceliumTheme
import mycelium.ui.ThreadView
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private const val Atlas = """{"id":"c2","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/atlas","title":"Release v0.3.7 checklist","status":"input","acp_session_id":"s","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:05:00Z"}"""
private const val Caller = """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/app","title":"Release captain","status":"review","acp_session_id":"s","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:05:00Z"}"""
private const val AtlasEvents = """[{"conversation_id":"c2","seq":1,"kind":"acp_update","payload":{"sessionId":"s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"The changelog and version bump are done."}}}},
{"conversation_id":"c2","seq":2,"kind":"permission_request","payload":{"request_id":"r1","toolCall":{"title":"mcp__gitlab__create_merge_request","server":"GitLab"},"options":[{"optionId":"approve","name":"Approve","kind":"allow_once"},{"optionId":"deny","name":"Deny","kind":"reject_once"}]}}]"""

class BroughtInConversationTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val posts = java.util.concurrent.CopyOnWriteArrayList<Pair<String, String>>()

    private val opened = java.util.concurrent.CopyOnWriteArrayList<String>()

    private fun showTheBroughtInChat(): Store {
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val body = String(request.body.toByteArray())

            if (request.method == HttpMethod.Post) {
                posts += path to body
            }

            val response = when {
                path.endsWith("/conversations") -> "[$Caller,$Atlas]"
                path.endsWith("/conversations/c2/events") -> AtlasEvents
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

                CompositionLocalProvider(LocalBroughtIn provides BroughtInActions(store, state, { opened += it })) {
                    Box(Modifier.width(720.dp)) {
                        ThreadView(listOf(ThreadItem.BroughtIn("e1", "c2", "Release v0.3.7 checklist")), onAnswer = { _, _ -> }, onStopWorkflow = {})
                    }
                }
            }
        }
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Approve mcp__gitlab__create_merge_request").fetchSemanticsNodes().isNotEmpty() }

        return store
    }

    @Test
    fun theBroughtInChatShowsItsStateItsLatestMessageAndItsPendingApproval() {
        showTheBroughtInChat()

        rule.onNodeWithContentDescription("Chat: atlas, Release v0.3.7 checklist").assertExists()
        rule.onNodeWithText("Needs input").assertExists()
        rule.onNodeWithText("The changelog and version bump are done.").assertExists()
        rule.onNodeWithContentDescription("Deny mcp__gitlab__create_merge_request").assertExists()
        rule.onNodeWithText("Reply in Release v0.3.7 checklist").assertExists()
        rule.onNodeWithContentDescription("Send to Release v0.3.7 checklist").assertExists()
        rule.onNodeWithText("Sent as you, straight into the chat").assertExists()
        rule.captureScreen("7.20-brought-in-conversation")
    }

    @Test
    fun openChatOpensTheBroughtInConversation() {
        showTheBroughtInChat()

        rule.onNodeWithContentDescription("Open the chat atlas, Release v0.3.7 checklist").performClick()

        assertEquals(listOf("c2"), opened.toList())
    }

    @Test
    fun approvingFromTheCardSendsTheAnswerTheChatSends() {
        showTheBroughtInChat()

        rule.onNodeWithContentDescription("Approve mcp__gitlab__create_merge_request").performClick()
        rule.waitUntil(5_000) { posts.any { it.first.endsWith("/conversations/c2/permission") } }

        assertEquals("""{"request_id":"r1","option_id":"approve"}""", posts.single { it.first.endsWith("/conversations/c2/permission") }.second.replace(" ", ""))
    }

    @Test
    fun aReplyFromTheCardIsSentUnchangedToThatChatsPromptRoute() {
        showTheBroughtInChat()

        rule.onNodeWithContentDescription("Reply in Release v0.3.7 checklist").performTextInput("Open the merge request")
        rule.onNodeWithContentDescription("Send to Release v0.3.7 checklist").performClick()
        rule.waitUntil(5_000) { posts.any { it.first.endsWith("/conversations/c2/prompt") } }

        val body = posts.single { it.first.endsWith("/conversations/c2/prompt") }.second
        assertTrue(body.contains("\"text\":\"Open the merge request\""), body)
    }

    @Test
    fun afterAReplyTheCardShowsTheReplyAndTheAnswerAndNoApproval() {
        val store = showTheBroughtInChat()

        rule.onNodeWithContentDescription("Reply in Release v0.3.7 checklist").performTextInput("Open the merge request")
        rule.onNodeWithContentDescription("Send to Release v0.3.7 checklist").performClick()
        rule.waitUntil(5_000) { posts.any { it.first.endsWith("/conversations/c2/prompt") } }

        store.receive(StreamMessage.EventAppended(Event("c2", 3, "permission_resolved", MyceliumJson.parseToJsonElement("""{"request_id":"r1"}"""))))
        store.receive(StreamMessage.EventAppended(Event("c2", 4, "user_message", MyceliumJson.parseToJsonElement("""{"text":"Open the merge request"}"""))))
        store.receive(StreamMessage.EventAppended(Event("c2", 5, "acp_update", MyceliumJson.parseToJsonElement("""{"sessionId":"s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Merge request opened as !42."}}}"""))))
        rule.waitForIdle()

        rule.onNodeWithText("Open the merge request").assertExists()
        rule.onNodeWithText("Merge request opened as !42.").assertExists()
        assertEquals(0, rule.onAllNodesWithContentDescription("Approve mcp__gitlab__create_merge_request").fetchSemanticsNodes().size)
        rule.captureScreen("7.21-brought-in-after-reply")
    }
}
