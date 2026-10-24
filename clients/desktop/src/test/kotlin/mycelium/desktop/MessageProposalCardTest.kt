package mycelium.desktop

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.unit.dp
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.serialization.json.Json
import kotlinx.serialization.serializer
import mycelium.api.Event
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.state.buildThread
import mycelium.ui.LocalMessageProposals
import mycelium.ui.MyTheme
import mycelium.ui.MyceliumTheme
import mycelium.ui.ThreadView
import mycelium.ui.messageProposals
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private const val ProposalId = "88888888-8888-8888-8888-888888888888"

private fun sendToConversation(): List<Event> {
    val output = Json.encodeToString(serializer<String>(), "Waiting for the person to approve sending this to Fix billing export.")
    val input = """{"conversation":"Fix billing export","message":"The schema is ready","reason":"You asked for the schema"}"""

    return listOf(
        Event("c1", 1, "acp_update", Json.parseToJsonElement("""{"sessionId":"s","update":{"sessionUpdate":"tool_call","toolCallId":"t1","title":"mcp__mycelium__send_to_conversation","kind":"other","status":"pending","rawInput":{}}}""")),
        Event("c1", 2, "acp_update", Json.parseToJsonElement("""{"sessionId":"s","update":{"sessionUpdate":"tool_call_update","toolCallId":"t1","status":"completed","rawInput":$input,"content":[{"type":"content","content":{"type":"text","text":$output}}]}}""")),
    )
}

private fun proposal(state: String) =
    """{"id":"$ProposalId","conversation_id":"c1","target_id":"c2","target_title":"Fix billing export","target_node":"laptop","target_project":"billing","message":"The schema is ready","reason":"You asked for the schema","state":"$state","created_at":"2026-10-23T10:00:00Z"}"""

class MessageProposalCardTest {
    @get:Rule
    val rule = createComposeRule()

    private val requests = java.util.Collections.synchronizedList(mutableListOf<Pair<HttpMethod, String>>())

    private fun show() {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            requests += request.method to path
            request.body.toByteArray()
            val body = when {
                path.endsWith("/connect") -> proposal("sent")
                path.endsWith("/conversation-connections") -> "[]"
                path.endsWith("/approve") -> proposal("sent")
                path.endsWith("/decline") -> proposal("declined")
                path.endsWith("/message-proposals") -> "[${proposal("proposed")}]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), CoroutineScope(Dispatchers.Unconfined))
        val items = buildThread(sendToConversation())

        rule.setContent {
            MyceliumTheme(true) {
                val state by store.state.collectAsState()

                CompositionLocalProvider(LocalMessageProposals provides messageProposals(store, state)) {
                    Box(Modifier.width(640.dp).background(MyTheme.colors.bgContent).padding(16.dp)) { ThreadView(items, onAnswer = { _, _ -> }, onStopWorkflow = {}) }
                }
            }
        }
    }

    private fun waitForRequest(method: HttpMethod, pathEnd: String) {
        rule.waitUntil(timeoutMillis = 5_000) { requests.toList().any { it.first == method && it.second.endsWith(pathEnd) } }
    }

    @Test
    fun theCardShowsTheTargetItsNodeAndProjectTheMessageAndTheReason() {
        show()

        rule.waitUntil(timeoutMillis = 5_000) { rule.onAllNodesWithText("Fix billing export").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("laptop · billing").assertExists()
        rule.onNodeWithText("The schema is ready").assertExists()
        rule.onNodeWithText("You asked for the schema").assertExists()
        rule.onNodeWithText("Send").assertExists()
        rule.onNodeWithText("Don't send").assertExists()
    }

    @Test
    fun pressingSendApprovesTheProposalAndTheCardShowsItWasSent() {
        show()
        rule.waitUntil(timeoutMillis = 5_000) { rule.onAllNodesWithText("Send").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithText("Send").performClick()

        waitForRequest(HttpMethod.Post, "/message-proposals/$ProposalId/approve")
        rule.waitUntil(timeoutMillis = 5_000) { rule.onAllNodesWithText("Sent").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Send").assertDoesNotExist()
        rule.onNodeWithText("Don't send").assertDoesNotExist()
        assertEquals(false, requests.any { it.second.endsWith("/decline") })
    }

    @Test
    fun pressingDontSendDeclinesTheProposalAndTheCardShowsItWasDeclined() {
        show()
        rule.waitUntil(timeoutMillis = 5_000) { rule.onAllNodesWithText("Don't send").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithText("Don't send").performClick()

        waitForRequest(HttpMethod.Post, "/message-proposals/$ProposalId/decline")
        rule.waitUntil(timeoutMillis = 5_000) { rule.onAllNodesWithText("Declined").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Send").assertDoesNotExist()
        assertEquals(false, requests.any { it.second.endsWith("/approve") })
    }
    @Test
    fun sendingAndConnectingApprovesFutureMessagesAsAnExplicitSeparateAction() {
        show()
        rule.waitUntil(timeoutMillis = 5_000) { rule.onAllNodesWithText("Send and connect").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Send and connect").performClick()
        waitForRequest(HttpMethod.Post, "/message-proposals/$ProposalId/connect")
        rule.waitUntil(timeoutMillis = 5_000) { rule.onAllNodesWithText("Sent").fetchSemanticsNodes().isNotEmpty() }
    }

}
