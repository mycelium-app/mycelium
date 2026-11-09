package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.performClick
import androidx.compose.ui.unit.dp
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.Json
import kotlinx.serialization.serializer
import androidx.compose.runtime.CompositionLocalProvider
import mycelium.api.Event
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.state.buildThread
import mycelium.ui.ArtifactAccessControls
import mycelium.ui.LocalArtifactAccessControls
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Shell
import mycelium.ui.ThreadView
import mycelium.ui.pendingRequest
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private const val RequestId = "00000000-0000-0000-0000-000000000001"

private const val Waiting = """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/harbour","title":"Bump the UI kit","status":"input","acp_session_id":null,"mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:05:00Z","archived_at":null}"""

private const val Request = """{"id":"$RequestId","artifact_id":"a1","conversation_id":"c1","permission":"edit","reason":"Continue the roadmap work","state":"requested","created_at":"2026-10-22T10:01:00Z"}"""

private fun accessRequested(firstSeq: Long = 1): List<Event> {
    val output = Json.encodeToString(serializer<String>(), "Requested edit access to artefact a1. The person can approve or decline request $RequestId in Artefacts. Wait for their decision before using it.")
    val input = """{"artifact":"a1","permission":"edit","reason":"Continue the roadmap work"}"""

    return listOf(
        Event("c1", firstSeq, "acp_update", Json.parseToJsonElement("""{"sessionId":"s","update":{"sessionUpdate":"tool_call","toolCallId":"t1","title":"mcp__mycelium__request_artifact_access","kind":"other","status":"pending","rawInput":{}}}""")),
        Event("c1", firstSeq + 1, "acp_update", Json.parseToJsonElement("""{"sessionId":"s","update":{"sessionUpdate":"tool_call_update","toolCallId":"t1","status":"completed","rawInput":$input,"content":[{"type":"content","content":{"type":"text","text":$output}}]}}""")),
    )
}

class ArtifactAccessRequestTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private fun engine(events: String) = MockEngine { request ->
        val path = request.url.encodedPath
        request.body.toByteArray()
        val body = when {
            path.endsWith("/artifact-access-requests") -> "[$Request]"
            path.endsWith("/inbox/feed") -> """{"posts":[]}"""
            path.endsWith("/privacy") -> """{"voice_enabled":false}"""
            path.endsWith("/conversations") -> "[$Waiting]"
            path.endsWith("/conversations/c1/events") -> events
            path.endsWith("/nodes") || path.endsWith("/groups") || path.endsWith("/workflows") || path.endsWith("/queue") || path.endsWith("/session-updates") -> "[]"
            else -> "{}"
        }

        respond(body, HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json"))
    }

    @Test
    fun aPendingArtifactAccessRequestAppearsInTheInboxNeedsYouWithItsDecisionButtons() {
        val events = Json.encodeToString(serializer<List<Event>>(), accessRequested())
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine(events)), scopes.create())

        runBlocking {
            store.refresh()
            store.refreshArtifactAccessRequests()
        }
        rule.setContent { MyceliumTheme(true) { Shell(store, AppsPlatform(), Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Inbox) } }
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Continue the roadmap work").fetchSemanticsNodes().isNotEmpty() }

        assertEquals(1, rule.onAllNodesWithText("Approve").fetchSemanticsNodes().size)
        assertEquals(1, rule.onAllNodesWithText("Decline").fetchSemanticsNodes().size)
    }
}
