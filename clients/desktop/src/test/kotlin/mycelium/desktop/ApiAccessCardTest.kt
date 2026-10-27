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
import androidx.compose.ui.test.onNodeWithContentDescription
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
import kotlinx.serialization.json.Json
import kotlinx.serialization.serializer
import mycelium.api.Event
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.state.apiScopeLabel
import mycelium.state.buildThread
import mycelium.ui.LocalApiAccessRequests
import mycelium.ui.MyTheme
import mycelium.ui.MyceliumTheme
import mycelium.ui.SessionsSettings
import mycelium.ui.ThreadView
import mycelium.ui.apiAccessRequests
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private const val RequestId = "99999999-9999-9999-9999-999999999999"

private fun requestApiAccess(): List<Event> {
    val output = Json.encodeToString(serializer<String>(), "Waiting for the person to approve API access (request $RequestId).")
    val input = """{"scopes":["conversations:read","nodes:read"],"hours":4,"reason":"Read the events to find the stuck turn"}"""

    return listOf(
        Event("c1", 1, "acp_update", Json.parseToJsonElement("""{"sessionId":"s","update":{"sessionUpdate":"tool_call","toolCallId":"t1","title":"mcp__mycelium__request_api_access","kind":"other","status":"pending","rawInput":{}}}""")),
        Event("c1", 2, "acp_update", Json.parseToJsonElement("""{"sessionId":"s","update":{"sessionUpdate":"tool_call_update","toolCallId":"t1","status":"completed","rawInput":$input,"content":[{"type":"content","content":{"type":"text","text":$output}}]}}""")),
    )
}

private fun accessRequest(state: String, scopes: String = """["conversations:read","nodes:read"]""") =
    """{"id":"$RequestId","conversation_id":"c1","scopes":$scopes,"hours":4,"reason":"Read the events to find the stuck turn","state":"$state","created_at":"2026-10-23T10:00:00Z"}"""

private data class Sent(val method: HttpMethod, val path: String, val body: String)

class ApiAccessCardTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val sent = java.util.Collections.synchronizedList(mutableListOf<Sent>())

    private fun show() {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            sent += Sent(request.method, path, String(request.body.toByteArray()))
            val body = when {
                path.endsWith("/approve") -> accessRequest("approved", """["conversations:read"]""")
                path.endsWith("/decline") -> accessRequest("declined")
                path.endsWith("/api-access-requests") -> "[${accessRequest("requested")}]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        val items = buildThread(requestApiAccess())

        rule.setContent {
            MyceliumTheme(true) {
                val state by store.state.collectAsState()

                CompositionLocalProvider(LocalApiAccessRequests provides apiAccessRequests(store, state)) {
                    Box(Modifier.width(640.dp).background(MyTheme.colors.bgContent).padding(16.dp)) { ThreadView(items, onAnswer = { _, _ -> }, onStopWorkflow = {}) }
                }
            }
        }
    }

    private fun waitForText(text: String) {
        rule.waitUntil(timeoutMillis = 5_000) { rule.onAllNodesWithText(text).fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun theCardShowsTheScopesTheDurationTheReasonAndApproveAndDecline() {
        show()

        waitForText("Approve")
        rule.onNodeWithText("Read conversations").assertExists()
        rule.onNodeWithText("Read nodes").assertExists()
        rule.onNodeWithText("For 4 hours").assertExists()
        rule.onNodeWithText("Read the events to find the stuck turn").assertExists()
        rule.onNodeWithText("Decline").assertExists()
    }

    @Test
    fun theAssistantsScopeReadsAsAllowManagingAssistants() {
        assertEquals("Allow managing assistants", apiScopeLabel("assistants"))
    }

    @Test
    fun untickingAScopeApprovesOnlyTheScopesLeftAndTheCardShowsItWasApproved() {
        show()
        waitForText("Approve")

        rule.onNodeWithContentDescription("Allow read nodes").performClick()
        rule.onNodeWithText("Approve").performClick()

        rule.waitUntil(timeoutMillis = 5_000) { sent.toList().any { it.path.endsWith("/api-access-requests/$RequestId/approve") } }
        val approval = sent.toList().first { it.path.endsWith("/approve") }
        assertEquals(HttpMethod.Post, approval.method)
        assertEquals("""{"scopes":["conversations:read"]}""", approval.body)
        waitForText("Approved")
        rule.onNodeWithText("Approve").assertDoesNotExist()
        rule.onNodeWithText("Decline").assertDoesNotExist()
    }

    @Test
    fun approvingWithEveryScopeUntickedSaysWhatToDoAndSendsNothing() {
        show()
        waitForText("Approve")

        rule.onNodeWithContentDescription("Allow read conversations").performClick()
        rule.onNodeWithContentDescription("Allow read nodes").performClick()
        rule.onNodeWithText("Approve").performClick()

        waitForText("Choose at least one kind of access to approve, or decline the request.")
        assertTrue(sent.toList().none { it.path.endsWith("/approve") })
    }

    @Test
    fun pressingDeclineDeclinesTheRequestAndTheCardShowsIt() {
        show()
        waitForText("Decline")

        rule.onNodeWithText("Decline").performClick()

        rule.waitUntil(timeoutMillis = 5_000) { sent.toList().any { it.path.endsWith("/api-access-requests/$RequestId/decline") } }
        waitForText("Declined")
        rule.onNodeWithText("Approve").assertDoesNotExist()
        assertTrue(sent.toList().none { it.path.endsWith("/approve") })
    }
}

class AgentSessionsTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val sent = java.util.Collections.synchronizedList(mutableListOf<Sent>())

    private fun show() {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            sent += Sent(request.method, path, "")
            val body = when {
                path.endsWith("/sessions") -> """[{"id":"t1","kind":"agent","label":"Agent access","hostname":"","address":"","last_used_at":"2026-10-23T09:00:00Z","current":false,"scopes":["conversations:read","nodes:read"],"expires_at":"2099-10-23T13:00:00Z","conversation_id":"c1","conversation_title":"Debug the sync"}]"""
                else -> "[]"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        rule.setContent { MyceliumTheme(true) { SessionsSettings(store, onSignOut = {}) } }
    }

    @Test
    fun anAgentTokenIsListedWithTheConversationThatAskedItsScopesAndItsExpiry() {
        show()

        rule.waitUntil(timeoutMillis = 5_000) { rule.onAllNodesWithText("Agent access for Debug the sync").fetchSemanticsNodes().isNotEmpty() }
        assertTrue(rule.onAllNodesWithText("Read conversations, Read nodes. Expires", substring = true).fetchSemanticsNodes().isNotEmpty())
    }

    @Test
    fun revokingAnAgentTokenEndsItAtOnce() {
        show()
        rule.waitUntil(timeoutMillis = 5_000) { rule.onAllNodesWithText("Agent access for Debug the sync").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithContentDescription("More for Agent access for Debug the sync").performClick()
        rule.onNodeWithText("Revoke").performClick()

        rule.waitUntil(timeoutMillis = 5_000) { rule.onAllNodesWithText("Agent access for Debug the sync").fetchSemanticsNodes().isEmpty() }
        assertEquals("/api/v1/sessions/t1", sent.toList().last { it.method == HttpMethod.Delete }.path)
    }
}
