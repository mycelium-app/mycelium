package mycelium.desktop

import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.test.assertHeightIsAtLeast
import androidx.compose.ui.test.getUnclippedBoundsInRoot
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
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
import java.awt.Toolkit
import java.awt.datatransfer.DataFlavor
import kotlin.io.encoding.Base64
import kotlinx.coroutines.runBlocking
import mycelium.api.Conversation
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.api.MyceliumJson
import mycelium.api.StreamMessage
import mycelium.state.Store
import mycelium.ui.LocalTouch
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Shell
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

internal const val ItemText = "Delete the merged branch from the remote. Only your key can push to acme-backups."

internal const val ItemCommand = "git push origin --delete feature/old-sync"

internal fun runJson(status: String, exitCode: Int? = null, output: String = "", reason: String? = null, startedAt: String = "2026-10-22T10:05:00Z") =
    """{"terminal_id":"t1","status":"$status","started_at":"$startedAt","output":"${output.replace("\n", "\\n")}"${exitCode?.let { ",\"exit_code\":$it" }.orEmpty()}${reason?.let { ",\"reason\":\"$it\"" }.orEmpty()}}"""

internal fun commandItemJson(run: String? = null) =
    """{"id":"i1","text":"$ItemText","created_at":"2026-10-22T10:01:00Z","command":"$ItemCommand","node_id":"n1","node_name":"atlas","folder":"~/projects/acme-backups"${run?.let { ",\"run\":$it" }.orEmpty()}}"""

internal fun commandConversation(item: String?) =
    """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work","title":"Tidy the merged branches","status":"done",
"acp_session_id":null,"mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:05:00Z","archived_at":null,"action_items":[${item.orEmpty()}]}"""

class ActionItemTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val posts = java.util.concurrent.CopyOnWriteArrayList<Pair<String, String>>()

    private fun show(item: String, phone: Boolean = false, runAnswer: Pair<HttpStatusCode, String>? = null): Store {
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            if (request.method == HttpMethod.Post) {
                posts += path to String(request.body.toByteArray())
            }

            val (status, body) = when {
                path.endsWith("/run") -> runAnswer ?: (HttpStatusCode.OK to commandConversation(commandItemJson(runJson("running"))))
                path.endsWith("/inbox/feed") -> HttpStatusCode.OK to """{"posts":[]}"""
                path.endsWith("/privacy") -> HttpStatusCode.OK to """{"voice_enabled":false}"""
                path.endsWith("/conversations") -> HttpStatusCode.OK to "[${commandConversation(item)}]"
                path.endsWith("/start-proposals") || path.endsWith("/message-proposals") || path.endsWith("/assistants") || path.endsWith("/nodes") || path.endsWith("/groups") || path.endsWith("/workflows") || path.endsWith("/queue") || path.endsWith("/session-updates") || path.endsWith("/conversations/c1/events") -> HttpStatusCode.OK to "[]"
                else -> HttpStatusCode.OK to "{}"
            }

            respond(body, status, headersOf(HttpHeaders.ContentType, "application/json"))
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        runBlocking { store.refresh() }
        rule.setContent { CompositionLocalProvider(LocalTouch provides phone) { MyceliumTheme(true) { Shell(store, AppsPlatform(), Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Inbox) } } }
        rule.waitUntil(5_000) { rule.onAllNodesWithText(ItemText).fetchSemanticsNodes().isNotEmpty() }

        return store
    }

    private fun receive(store: Store, item: String?) {
        store.receive(StreamMessage.ConversationChanged(MyceliumJson.decodeFromString(Conversation.serializer(), commandConversation(item))))
        rule.waitForIdle()
    }

    @Test
    fun aWaitingCommandOffersRunFirstThenMarkAsDone() {
        show(commandItemJson())

        rule.onNodeWithText("Resolves when the command succeeds").assertExists()
        rule.onNodeWithText("atlas").assertExists()
        rule.onNodeWithText("~/projects/acme-backups").assertExists()

        val run = rule.onNodeWithContentDescription("Run $ItemCommand on atlas")
        val markDone = rule.onNodeWithContentDescription("Mark as done: $ItemText")

        assertTrue(run.getUnclippedBoundsInRoot().left < markDone.getUnclippedBoundsInRoot().left)
    }

    @Test
    fun copyPutsTheCommandOnTheClipboard() {
        show(commandItemJson())

        rule.onNodeWithContentDescription("Copy the command").performClick()
        rule.waitForIdle()

        assertEquals(ItemCommand, Toolkit.getDefaultToolkit().systemClipboard.getData(DataFlavor.stringFlavor))
    }

    @Test
    fun runningTheCommandPostsTheTerminalSizeAndStreamsItsOutput() {
        val store = show(commandItemJson())

        rule.onNodeWithContentDescription("Run $ItemCommand on atlas").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Running").fetchSemanticsNodes().isNotEmpty() }

        assertEquals("""{"cols":80,"rows":24}""", posts.single { it.first.endsWith("/action-items/i1/run") }.second)
        rule.onNodeWithContentDescription("Stop $ItemCommand").assertExists()
        rule.onNodeWithContentDescription("Open in terminal: $ItemCommand").assertExists()

        store.receive(StreamMessage.TerminalOutput("t1", Base64.encode("To git.example.com:acme/acme-backups.git\r\n".encodeToByteArray())))
        rule.waitForIdle()

        rule.onNodeWithContentDescription("Output of $ItemCommand").assertExists()
        rule.onNodeWithText("To git.example.com:acme/acme-backups.git").assertExists()
    }

    @Test
    fun aFailedRunShowsItsExitCodeAndOffersRetryMarkAsDoneAndTheTerminal() {
        show(commandItemJson(runJson("failed", exitCode = 1, output = "error: unable to delete 'feature/old-sync'")))

        rule.onNodeWithText("Failed").assertExists()
        rule.onNodeWithText("Exit code 1").assertExists()
        rule.onNodeWithText("error: unable to delete 'feature/old-sync'").assertExists()
        rule.onNodeWithContentDescription("Retry $ItemCommand on atlas").assertExists()
        rule.onNodeWithContentDescription("Mark as done: $ItemText").assertExists()
        rule.onNodeWithContentDescription("Open in terminal: $ItemCommand").assertExists()
    }

    @Test
    fun aRunThatCouldNotStartShowsTheServersReasonOnTheCard() {
        show(commandItemJson(), runAnswer = HttpStatusCode.Conflict to """{"error":"node_offline","message":"atlas is offline. Bring it online and try again."}""")

        rule.onNodeWithContentDescription("Run $ItemCommand on atlas").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Failed").fetchSemanticsNodes().isNotEmpty() }

        assertTrue(rule.onAllNodesWithText("atlas is offline. Bring it online and try again.").fetchSemanticsNodes().isNotEmpty())
        rule.onNodeWithContentDescription("Retry $ItemCommand on atlas").assertExists()
    }

    @Test
    fun aSucceededRunStaysVisibleAfterTheServerResolvesItThenLeaves() {
        val store = show(commandItemJson(runJson("running")))

        store.receive(StreamMessage.TerminalExited("t1", 0))
        rule.waitForIdle()

        rule.onNodeWithText("Succeeded").assertExists()
        rule.onNodeWithText("Exit code 0").assertExists()

        receive(store, null)

        rule.onNodeWithText("Succeeded").assertExists()
        rule.onNodeWithText("It leaves Needs you.").assertExists()
        assertEquals(0, rule.onAllNodesWithContentDescription("Stop $ItemCommand").fetchSemanticsNodes().size)

        rule.mainClock.advanceTimeBy(5_000)
        rule.waitForIdle()

        rule.onNodeWithText("Nothing needs you").assertExists()
    }

    @Test
    fun markingACommandItemDoneKeepsTheCommandMutedWithoutCopy() {
        val store = show(commandItemJson())

        rule.onNodeWithContentDescription("Mark as done: $ItemText").performClick()
        receive(store, null)

        rule.onNodeWithText("Marked as done. It leaves Needs you.").assertExists()
        rule.onNodeWithText(ItemCommand).assertExists()
        assertEquals(0, rule.onAllNodesWithContentDescription("Copy the command").fetchSemanticsNodes().size)
    }

    @Test
    fun onAPhoneTheRunButtonIsAtLeast44DpTall() {
        show(commandItemJson(), phone = true)

        rule.onNodeWithContentDescription("Run $ItemCommand on atlas").assertHeightIsAtLeast(44.dp)
    }
}
