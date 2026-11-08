package mycelium.desktop

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlin.time.Instant
import kotlinx.coroutines.runBlocking
import mycelium.api.Activity
import mycelium.api.ActivityRow
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.api.MyceliumJson
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Shell

import mycelium.ui.tokenRate
import mycelium.ui.tokenSample
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private fun conversation(status: String) = """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/harbour","title":"Bump the UI kit","status":"$status",
"acp_session_id":null,"mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:05:00Z","archived_at":null}"""

private const val PermissionEvents = """[{"conversation_id":"c1","seq":1,"kind":"permission_request","payload":{"request_id":"r1","toolCall":{"title":"pnpm db:migrate"},
"options":[{"optionId":"approve","name":"Approve","kind":"allow_once"},{"optionId":"deny","name":"Deny","kind":"reject_once"}]}}]"""

class InboxTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val posts = java.util.Collections.synchronizedList(mutableListOf<Pair<String, String>>())

    private fun show(status: String): Store {
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            if (request.method == HttpMethod.Post) {
                posts += path to String(request.body.toByteArray())
            }

            val body = when {
                path.endsWith("/conversations") -> "[${conversation(status)}]"
                path.endsWith("/conversations/c1/events") -> if (status == "input") PermissionEvents else "[]"
                path.endsWith("/nodes") || path.endsWith("/groups") || path.endsWith("/workflows") || path.endsWith("/queue") || path.endsWith("/session-updates") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json"))
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { Shell(store, AppsPlatform(), Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Inbox) } }

        return store
    }

    @Test
    fun aPermissionFromAConversationAwaitingInputIsApprovedFromTheInbox() {
        show("input")
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Approve pnpm db:migrate").fetchSemanticsNodes().isNotEmpty() }

        assertTrue(rule.onAllNodesWithText("Bump the UI kit").fetchSemanticsNodes().isNotEmpty())
        rule.onNodeWithText("Approve").performClick()

        rule.waitUntil(5_000) { posts.any { it.first.endsWith("/conversations/c1/permission") } }
        assertTrue(posts.single { it.first.endsWith("/conversations/c1/permission") }.second.contains("\"approve\""))
    }

    @Test
    fun anApprovedItemStaysResolvedBrieflyThenLeaves() {
        val store = show("input")
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Approve pnpm db:migrate").fetchSemanticsNodes().isNotEmpty() }

        store.receive(mycelium.api.StreamMessage.EventAppended(mycelium.api.Event("c1", 2, "permission_resolved", MyceliumJson.parseToJsonElement("""{"request_id":"r1"}"""))))
        store.receive(mycelium.api.StreamMessage.ConversationChanged(MyceliumJson.decodeFromString(mycelium.api.Conversation.serializer(), conversation("working"))))
        rule.waitForIdle()

        rule.onNodeWithText("Answered").assertExists()

        rule.mainClock.advanceTimeBy(5_000)
        rule.waitForIdle()

        rule.onNodeWithText("Nothing needs you").assertExists()
    }

    @Test
    fun nothingPendingSaysSo() {
        show("done")

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Nothing needs you").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun theSidebarLinkCountsWhatNeedsYou() {
        show("input")

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("1 waiting").fetchSemanticsNodes().isNotEmpty() }
        assertTrue(rule.onAllNodesWithText("Inbox").fetchSemanticsNodes().isNotEmpty())
    }

    @Test
    fun theSidebarLinkShowsNoCountWhenNothingNeedsYou() {
        show("done")

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Nothing needs you").fetchSemanticsNodes().isNotEmpty() }
        assertEquals(0, rule.onAllNodesWithContentDescription("0 waiting").fetchSemanticsNodes().size)
    }

    @Test
    fun tokensPerSecondIsTheTokenDeltaOverElapsedTime() {
        fun activity(tokens: Long, running: Long) = Activity("today", ActivityRow("total", "Total", inputTokens = tokens), conversations = listOf(ActivityRow("c1", "Build", inputTokens = running)))

        val first = tokenSample(activity(1_000, 100), Instant.parse("2026-10-22T10:00:00Z"))
        val second = tokenSample(activity(1_600, 400), Instant.parse("2026-10-22T10:00:15Z"))
        val rate = tokenRate(first, second, activity(1_600, 400), earlierPeak = 0)

        assertEquals(40L, rate.perSecond)
        assertEquals(20L, rate.running.getValue("c1"))
        assertEquals(40L, rate.peak)
    }
}
