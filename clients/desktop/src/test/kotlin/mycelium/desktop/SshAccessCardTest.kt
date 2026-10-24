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
import mycelium.ui.LocalSshAccessRequests
import mycelium.ui.MyTheme
import mycelium.ui.MyceliumTheme
import mycelium.ui.ThreadView
import mycelium.ui.sshAccessRequests
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private const val SshRequestId = "88888888-8888-8888-8888-888888888888"

private fun requestSshAccess(): List<Event> {
    val output = Json.encodeToString(serializer<String>(), "Waiting for the person to approve SSH access to build box (request $SshRequestId).")
    val input = """{"node":"build box","minutes":60,"reason":"Find out why the disk is full"}"""

    return listOf(
        Event("c1", 1, "acp_update", Json.parseToJsonElement("""{"sessionId":"s","update":{"sessionUpdate":"tool_call","toolCallId":"t1","title":"mcp__mycelium__request_ssh_access","kind":"other","status":"pending","rawInput":{}}}""")),
        Event("c1", 2, "acp_update", Json.parseToJsonElement("""{"sessionId":"s","update":{"sessionUpdate":"tool_call_update","toolCallId":"t1","status":"completed","rawInput":$input,"content":[{"type":"content","content":{"type":"text","text":$output}}]}}""")),
    )
}

private fun sshRequest(state: String, minutes: Int = 60, expiresAt: String? = null) =
    """{"id":"$SshRequestId","conversation_id":"c1","node_id":"n1","node_name":"build box","minutes":$minutes,"reason":"Find out why the disk is full","state":"$state","created_at":"2026-10-23T10:00:00Z"${expiresAt?.let { ""","expires_at":"$it"""" } ?: ""}}"""

private data class SshSent(val method: HttpMethod, val path: String, val body: String)

class SshAccessCardTest {
    @get:Rule
    val rule = createComposeRule()

    private val sent = java.util.Collections.synchronizedList(mutableListOf<SshSent>())

    private fun show() {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            sent += SshSent(request.method, path, String(request.body.toByteArray()))
            val body = when {
                path.endsWith("/approve") -> sshRequest("approved", 15, "2026-10-23T10:15:00Z")
                path.endsWith("/decline") -> sshRequest("declined")
                path.endsWith("/revoke") -> sshRequest("revoked", 15, "2026-10-23T10:15:00Z")
                path.endsWith("/ssh-access-requests") -> "[${sshRequest("requested")}]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), CoroutineScope(Dispatchers.Unconfined))
        val items = buildThread(requestSshAccess())

        rule.setContent {
            MyceliumTheme(true) {
                val state by store.state.collectAsState()

                CompositionLocalProvider(LocalSshAccessRequests provides sshAccessRequests(store, state)) {
                    Box(Modifier.width(640.dp).background(MyTheme.colors.bgContent).padding(16.dp)) { ThreadView(items, onAnswer = { _, _ -> }, onStopWorkflow = {}) }
                }
            }
        }
    }

    private fun waitForText(text: String) {
        rule.waitUntil(timeoutMillis = 5_000) { rule.onAllNodesWithText(text).fetchSemanticsNodes().isNotEmpty() }
    }

    private fun waitForCall(suffix: String) {
        rule.waitUntil(timeoutMillis = 5_000) { sent.toList().any { it.path.endsWith(suffix) } }
    }

    @Test
    fun theCardShowsTheNodeTheReasonTheDurationChoicesAndApproveAndDecline() {
        show()

        waitForText("Approve")
        rule.onNodeWithText("Node build box").assertExists()
        rule.onNodeWithText("Find out why the disk is full").assertExists()
        rule.onNodeWithText("15 minutes").assertExists()
        rule.onNodeWithText("30 minutes").assertExists()
        rule.onNodeWithText("1 hour").assertExists()
        rule.onNodeWithText("2 hours").assertDoesNotExist()
        rule.onNodeWithText("Decline").assertExists()
    }

    @Test
    fun choosingAShorterDurationApprovesThatLongAndTheCardOffersRevoke() {
        show()
        waitForText("Approve")

        rule.onNodeWithText("15 minutes").performClick()
        rule.onNodeWithText("Approve").performClick()

        waitForCall("/ssh-access-requests/$SshRequestId/approve")
        val approval = sent.toList().first { it.path.endsWith("/approve") }
        assertEquals(HttpMethod.Post, approval.method)
        assertEquals("""{"minutes":15}""", approval.body)
        waitForText("Approved")
        waitForText("Revoke")
        rule.onNodeWithText("Until 10:15 UTC").assertExists()
        rule.onNodeWithText("Decline").assertDoesNotExist()
    }

    @Test
    fun revokingApprovedAccessShowsItWasRevoked() {
        show()
        waitForText("Approve")
        rule.onNodeWithText("15 minutes").performClick()
        rule.onNodeWithText("Approve").performClick()
        waitForText("Revoke")

        rule.onNodeWithText("Revoke").performClick()

        waitForCall("/ssh-access-requests/$SshRequestId/revoke")
        waitForText("Revoked")
        rule.onNodeWithText("Revoke").assertDoesNotExist()
    }

    @Test
    fun pressingDeclineDeclinesTheRequestAndTheCardShowsIt() {
        show()
        waitForText("Decline")

        rule.onNodeWithText("Decline").performClick()

        waitForCall("/ssh-access-requests/$SshRequestId/decline")
        waitForText("Declined")
        rule.onNodeWithText("Approve").assertDoesNotExist()
        assertTrue(sent.toList().none { it.path.endsWith("/approve") })
    }
}
