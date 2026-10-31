package mycelium.desktop

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import java.util.Base64
import java.util.concurrent.CopyOnWriteArrayList
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import mycelium.api.Conversation
import mycelium.api.MyceliumApi
import mycelium.api.MyceliumJson
import mycelium.state.AppState
import mycelium.state.SplitDirection
import mycelium.state.Store
import mycelium.state.TerminalTabs
import mycelium.state.TerminalWorkspace
import mycelium.state.paneIds
import mycelium.ui.ConversationScreen
import mycelium.ui.LocalTouch
import mycelium.ui.MyceliumTheme
import mycelium.ui.TerminalPanel
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private const val PhoneConversationJson = """{"id":"c1","owner":"u","node_id":"n","agent":"claude","cwd":"/work/midden","title":"Bump","status":"working",
"acp_session_id":null,"mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null}"""

class TerminalPhoneTest {
    @get:Rule
    val scopes = TestScopes()

    @get:Rule
    val compose = createComposeRule()

    private val sent = CopyOnWriteArrayList<String>()
    private var openTerminals = "[]"
    private val json = headersOf(HttpHeaders.ContentType, "application/json")

    private fun store(): Store {
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            if (path.endsWith("/input")) {
                sent += String(Base64.getDecoder().decode(Json.parseToJsonElement(String(request.body.toByteArray())).jsonObject.getValue("data").jsonPrimitive.content))
            }

            when {
                path.endsWith("/conversations") -> respond("[$PhoneConversationJson]", HttpStatusCode.OK, json)
                path.endsWith("/conversations/c1") -> respond(PhoneConversationJson, HttpStatusCode.OK, json)
                path.endsWith("/terminal") -> respond("""{"terminal_id":"t1"}""", HttpStatusCode.OK, json)
                path.endsWith("/terminals") -> respond(openTerminals, HttpStatusCode.OK, json)
                path.endsWith("/events") || path.endsWith("/queue") || path.endsWith("/nodes") -> respond("[]", HttpStatusCode.OK, json)
                else -> respond("{}", HttpStatusCode.OK, json)
            }
        }

        return Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
    }

    private fun showPanel(workspace: TerminalWorkspace? = null) {
        val conversation = MyceliumJson.decodeFromString(Conversation.serializer(), PhoneConversationJson)
        val store = store()

        if (workspace != null) {
            store.terminalWorkspaces["c1"] = workspace
        }

        compose.setContent {
            MyceliumTheme(true) {
                CompositionLocalProvider(LocalTouch provides true) {
                    TerminalPanel(store, AppState(), conversation, Modifier.fillMaxSize())
                }
            }
        }

        compose.waitUntil(5_000) { compose.onAllNodesWithContentDescription("Type into terminal").fetchSemanticsNodes().isNotEmpty() }
    }

    private fun waitForSent(count: Int) = compose.waitUntil(5_000) { sent.joinToString("").length >= count }

    @Test
    fun theTouchTerminalHasNoSplitButtons() {
        showPanel()

        compose.onNodeWithContentDescription("Split right").assertDoesNotExist()
        compose.onNodeWithContentDescription("Split down").assertDoesNotExist()
        compose.onNodeWithContentDescription("New terminal").assertExists()
        compose.onNodeWithContentDescription("Close the terminal").assertExists()
    }

    @Test
    fun aSplitWorkspaceShowsOneTerminalAtATimeAndListsEveryPane() {
        val split = TerminalTabs().open().split(SplitDirection.Right)
        val panes = split.current!!.layout.paneIds()
        openTerminals = """[{"terminal_id":"t1","scrollback":""},{"terminal_id":"t2","scrollback":""}]"""

        showPanel(TerminalWorkspace(split, mapOf(panes[0] to "t1", panes[1] to "t2")))

        compose.onNodeWithContentDescription("Resize the split").assertDoesNotExist()
        compose.onNodeWithContentDescription("Terminal 1, pane 2 of 2, switch terminal").performClick()
        compose.onNodeWithText("Terminal 1, pane 1").assertExists()
        compose.onNodeWithContentDescription("End Terminal 1, pane 2").assertExists()
        compose.onNodeWithText("Terminal 1, pane 1").performClick()

        compose.onNodeWithContentDescription("Terminal 1, pane 1 of 2, switch terminal").assertExists()
    }

    @Test
    fun typedTextReachesTheTerminal() {
        showPanel()

        compose.onNodeWithContentDescription("Type into terminal").performTextInput("ls")
        waitForSent(2)

        assertEquals("ls", sent.joinToString(""))
    }

    @Test
    fun ctrlThenCSendsTheInterruptAndTurnsCtrlOff() {
        showPanel()

        compose.onNodeWithText("Ctrl").performClick()
        compose.onNodeWithContentDescription("Type into terminal").performTextInput("c")
        waitForSent(1)
        compose.onNodeWithContentDescription("Type into terminal").performTextInput("c")
        waitForSent(2)

        assertEquals("\u0003c", sent.joinToString(""))
    }

    @Test
    fun theArrowKeysSendCursorSequences() {
        showPanel()

        listOf("Left", "Up", "Down", "Right").forEach { compose.onNodeWithContentDescription(it).performClick() }
        waitForSent(12)

        assertEquals("\u001B[D\u001B[A\u001B[B\u001B[C", sent.joinToString(""))
    }

    @Test
    fun theTerminalSheetHidesTheComposer() {
        val store = store()

        runBlocking { store.refresh(); store.open("c1") }
        compose.setContent {
            MyceliumTheme(true) {
                CompositionLocalProvider(LocalTouch provides true) {
                    val state by store.state.collectAsState()

                    ConversationScreen(store, state, "c1", onArchived = {}, panels = false)
                }
            }
        }
        compose.waitUntil(5_000) { compose.onAllNodes(hasSetTextAction()).fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithContentDescription("Show terminal").performClick()
        compose.waitUntil(5_000) { compose.onAllNodesWithContentDescription("Type into terminal").fetchSemanticsNodes().isNotEmpty() }

        compose.onAllNodes(hasSetTextAction() and hasContentDescription("Type into terminal").not()).assertCountEquals(0)
    }
}
