package mycelium.desktop

import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.requestFocus
import androidx.compose.ui.test.runDesktopComposeUiTest
import androidx.compose.ui.test.ComposeUiTest
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import kotlinx.coroutines.runBlocking
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.InboxScreen
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Shell
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private const val Projects = """[{"id":"p1","node_id":"n1","name":"harbour","path":"/work/harbour","created_at":"2026-10-22T10:00:00Z"},{"id":"p2","node_id":"n1","name":"receipts","path":"/work/receipts","created_at":"2026-10-22T10:00:00Z"}]"""

private const val FlagReason = "Three of you had the set menu and one had the tasting menu. Should the 12.5% service charge be split evenly, or in proportion to what each person ordered?"

private fun inboxConversation(id: String, project: String, title: String, status: String, flagged: Boolean) =
    """{"id":"$id","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/$project","title":"$title","status":"$status","project_id":"${if (project == "harbour") "p1" else "p2"}",
"acp_session_id":null,"mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:05:00Z","archived_at":null${if (flagged) ""","flagged_at":"2026-10-22T10:06:00Z","flag_reason":"$FlagReason"""" else ""}}"""

private const val Approval = """[{"conversation_id":"c1","seq":1,"kind":"permission_request","payload":{"request_id":"r1","toolCall":{"title":"pnpm db:migrate"},"options":[{"optionId":"approve","name":"Approve","kind":"allow_once"},{"optionId":"deny","name":"Deny","kind":"reject_once"}]}}]"""

private const val ReceiptMessage = """[{"conversation_id":"c2","seq":1,"kind":"acp_update","payload":{"sessionId":"s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Split the receipt from Saturday between Sam, Alex, Priya and me."}}}}]"""

@OptIn(ExperimentalTestApi::class)
class InboxFlagTest {
    @get:Rule
    val scopes = TestScopes()

    private val patches = java.util.concurrent.CopyOnWriteArrayList<Pair<String, String>>()

    private val opened = java.util.concurrent.CopyOnWriteArrayList<String>()

    private fun store(): Store {
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            if (request.method == HttpMethod.Patch) {
                patches += path to String(request.body.toByteArray())

                return@MockEngine respond(inboxConversation("c2", "receipts", "Restaurant receipt split breakdown", "done", flagged = false), HttpStatusCode.OK, JsonHeaders)
            }

            val body = when {
                path.endsWith("/conversations") -> "[${inboxConversation("c1", "harbour", "Bump the UI kit to 0.31.2", "input", flagged = false)},${inboxConversation("c2", "receipts", "Restaurant receipt split breakdown", "done", flagged = true)}]"
                path.endsWith("/projects") -> Projects
                path.endsWith("/conversations/c1/events") -> Approval
                path.endsWith("/conversations/c2/events") -> ReceiptMessage
                path.endsWith("/inbox/feed") -> """{"posts":[]}"""
                path.endsWith("/privacy") -> """{"voice_enabled":false}"""
                path.endsWith("/nodes") || path.endsWith("/groups") || path.endsWith("/workflows") || path.endsWith("/queue") || path.endsWith("/session-updates") || path.endsWith("/events") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, JsonHeaders)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        runBlocking {
            store.refresh()
            store.refreshProjects()
        }

        return store
    }

    private fun ComposeUiTest.showInbox() {
        val store = store()

        setContent {
            val state by store.state.collectAsState()

            MyceliumTheme(true) { InboxScreen(store, state, onOpenConversation = { opened += it }, onOpenUsageHistory = {}) }
        }
        waitUntil(timeoutMillis = 5_000) { onAllNodesWithContentDescription("receipts Restaurant receipt split breakdown").fetchSemanticsNodes().isNotEmpty() }
    }

    private fun clearedFlags() = patches.filter { it.second.contains("\"clear_flag\":true") }

    @Test
    fun openConversationOnAFlaggedItemOpensTheChatAndClearsTheFlag() = runDesktopComposeUiTest(width = 1440, height = 900) {
        showInbox()

        onNodeWithText("Open conversation").performClick()

        waitUntil(timeoutMillis = 5_000) { clearedFlags().isNotEmpty() }
        assertEquals(listOf("c2"), opened.toList())
        assertEquals(listOf("/api/v1/conversations/c2"), clearedFlags().map { it.first })
    }

    @Test
    fun dismissOnAFlaggedItemClearsTheFlagAndOpensNothing() = runDesktopComposeUiTest(width = 1440, height = 900) {
        showInbox()

        onNodeWithText("Dismiss").performClick()

        waitUntil(timeoutMillis = 5_000) { clearedFlags().isNotEmpty() }
        assertEquals(emptyList(), opened.toList())
    }

    @Test
    fun theChevronOnlyExpandsAndCollapsesAnItem() = runDesktopComposeUiTest(width = 1440, height = 900) {
        showInbox()

        onNodeWithContentDescription("Show more of Restaurant receipt split breakdown").performClick()
        onNodeWithContentDescription("Show less of Restaurant receipt split breakdown").performClick()
        waitForIdle()

        assertEquals(emptyList(), opened.toList())
        assertEquals(emptyList(), patches.toList())
    }

    @Test
    fun theTitleLinkOfAFlaggedItemOpensTheChatAndClearsTheFlag() = runDesktopComposeUiTest(width = 1440, height = 900) {
        showInbox()

        onNodeWithContentDescription("receipts Restaurant receipt split breakdown").performClick()

        waitUntil(timeoutMillis = 5_000) { clearedFlags().isNotEmpty() }
        assertEquals(listOf("c2"), opened.toList())
    }

    @Test
    fun theTitleLinkOfAnItemThatIsNotFlaggedOnlyOpensTheChat() = runDesktopComposeUiTest(width = 1440, height = 900) {
        showInbox()

        onNodeWithContentDescription("harbour Bump the UI kit to 0.31.2").performClick()
        waitForIdle()

        assertEquals(listOf("c1"), opened.toList())
        assertEquals(emptyList(), patches.toList())
    }

    @Test
    fun screenshot526And539TheInboxWithAFlaggedItem() = runDesktopComposeUiTest(width = 1440, height = 1530) {
        val store = store()

        setContent { MyceliumTheme(true) { Shell(store, AppsPlatform(), Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Inbox) } }
        waitUntil(timeoutMillis = 5_000) { onAllNodesWithContentDescription("receipts Restaurant receipt split breakdown").fetchSemanticsNodes().isNotEmpty() }
        saveShot("5.26-inbox")

        onNodeWithContentDescription("Show more of Restaurant receipt split breakdown").performClick()
        onNodeWithContentDescription("receipts Restaurant receipt split breakdown").requestFocus()
        saveShot("5.39-inbox-flagged-item-expanded")
        assertTrue(java.io.File("/tmp/mycelium-build-flag-adopt/shots/5.39-inbox-flagged-item-expanded.png").length() > 10_000)
    }
}
