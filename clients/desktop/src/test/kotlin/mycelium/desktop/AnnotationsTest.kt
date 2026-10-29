package mycelium.desktop

import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.focus.FocusManager
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performMouseInput
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.test.assertTextEquals

import androidx.compose.ui.test.pressKey
import androidx.compose.ui.platform.ClipboardManager
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.text.AnnotatedString
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.runBlocking
import mycelium.api.Conversation
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.Route
import mycelium.ui.Shell
import mycelium.api.ConversationStatus
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.ui.unit.dp
import mycelium.state.Annotation
import mycelium.state.DiffLine
import mycelium.state.FileDiff
import mycelium.state.ToolStatus
import mycelium.state.SessionOptions
import mycelium.state.ThreadItem
import mycelium.ui.AnnotateActions
import mycelium.ui.Composer
import mycelium.ui.LocalAnnotate
import mycelium.ui.MyceliumTheme
import mycelium.ui.ThreadView
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private val holdsOneCitationMarker = SemanticsMatcher("holds one citation marker") { it.config.getOrNull(SemanticsProperties.EditableText)?.text?.length == 1 }

class AnnotationsTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val conversation = Conversation("c1", "u", "n", "claude", "/work", "t", ConversationStatus.Done, createdAt = "2026-10-22T10:00:00Z", updatedAt = "2026-10-22T10:00:00Z")

    private fun composer(annotations: List<Annotation>, onSend: (String) -> Unit = {}, onRemove: (Annotation) -> Unit = {}) {
        rule.setContent {
            MyceliumTheme(true) {
                Composer(
                    conversation = conversation, working = false, queue = emptyList(), sending = emptyList(), onSend = onSend, onStop = {},
                    onSendQueuedNow = {}, onEditQueued = {}, onRemoveQueued = {},
                    options = SessionOptions(emptyList(), null, emptyList(), null, emptyList()), commands = emptyList(),
                    onSetMode = {}, onSetModel = {}, onSetSetting = { _, _ -> }, onSearchFiles = { emptyList() },
                    annotations = annotations, onRemoveAnnotation = onRemove,
                )
            }
        }
    }

    @Test
    fun aCitationDropsIntoTheComposerAndIsSentAsAQuoteAroundWhatIsTyped() {
        val removed = mutableListOf<Annotation>()
        val sent = mutableListOf<String>()
        val cited = Annotation("the retry delay", "")
        composer(listOf(cited), onSend = { sent += it }, onRemove = { removed += it })

        rule.waitForIdle()
        rule.onNode(hasSetTextAction()).performTextInput(" is too short")
        rule.onNodeWithContentDescription("Send").performClick()

        assertEquals(listOf(cited), removed)
        assertEquals(listOf("> the retry delay\n is too short"), sent)
    }

    private val memory = object : ClipboardManager {
        private var held: AnnotatedString? = AnnotatedString("RESTORE_BACKOFF_MS")

        override fun setText(annotatedString: AnnotatedString) {
            held = annotatedString
        }

        override fun getText(): AnnotatedString? = held
    }

    @Test
    fun selectingTextInAMessageOffersToCiteItInTheNextMessage() {
        val attached = mutableListOf<Annotation>()
        rule.setContent {
            MyceliumTheme(true) {
                CompositionLocalProvider(LocalAnnotate provides AnnotateActions(onAttach = { attached += it }, onAsk = { _, _ -> }), LocalClipboardManager provides memory) {
                    ThreadView(listOf(ThreadItem.AgentMessage("e1", "The retry delay is read from RESTORE_BACKOFF_MS")), onAnswer = { _, _ -> }, onStopWorkflow = {})
                }
            }
        }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("The retry delay is read from RESTORE_BACKOFF_MS").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("The retry delay is read from RESTORE_BACKOFF_MS").performMouseInput {
            moveTo(Offset(1f, centerY))
            press()
            moveTo(Offset(width * 0.5f, centerY))
            moveTo(Offset(width - 1f, centerY))
            release()
        }
        rule.onAllNodesWithText("Ctrl+Shift+A", substring = true).assertCountEquals(0)
        waitForOffer()
        rule.onNodeWithContentDescription("Cite, Ctrl+Shift+A").assertExists()
        waitForOffer()
        rule.onNodeWithText("Cite").assertIsDisplayed().performClick()

        assertEquals(listOf(Annotation("The retry delay is read from RESTORE_BACKOFF_MS", "")), attached)
    }

    @Test
    fun theOfferSurvivesTheMessageLosingFocus() {
        var focus: FocusManager? = null
        val items = listOf<ThreadItem>(ThreadItem.AgentMessage("e1", "The retry delay is read from RESTORE_BACKOFF_MS"))
        rule.setContent {
            MyceliumTheme(true) {
                CompositionLocalProvider(LocalAnnotate provides AnnotateActions(onAttach = { }, onAsk = { _, _ -> }), LocalClipboardManager provides memory) {
                    focus = LocalFocusManager.current
                    ThreadView(items, onAnswer = { _, _ -> }, onStopWorkflow = {})
                }
            }
        }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("The retry delay is read from RESTORE_BACKOFF_MS").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("The retry delay is read from RESTORE_BACKOFF_MS").performMouseInput {
            moveTo(Offset(1f, centerY))
            press()
            moveTo(Offset(width * 0.5f, centerY))
            moveTo(Offset(width - 1f, centerY))
            release()
        }
        waitForOffer()
        rule.onNodeWithText("Cite").assertIsDisplayed()

        rule.runOnIdle { focus!!.clearFocus() }
        rule.waitForIdle()

        waitForOffer()
        rule.onNodeWithText("Cite").assertIsDisplayed()
    }

    @Test
    fun theOfferGoesWhenTheNextSelectionStartsElsewhere() {
        val items = listOf<ThreadItem>(ThreadItem.AgentMessage("e1", "The retry delay is read from RESTORE_BACKOFF_MS"), ThreadItem.AgentMessage("e2", "Another message"))
        rule.setContent {
            MyceliumTheme(true) {
                CompositionLocalProvider(LocalAnnotate provides AnnotateActions(onAttach = { }, onAsk = { _, _ -> }), LocalClipboardManager provides memory) {
                    ThreadView(items, onAnswer = { _, _ -> }, onStopWorkflow = {})
                }
            }
        }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("The retry delay is read from RESTORE_BACKOFF_MS").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("The retry delay is read from RESTORE_BACKOFF_MS").performMouseInput {
            moveTo(Offset(1f, centerY))
            press()
            moveTo(Offset(width * 0.5f, centerY))
            moveTo(Offset(width - 1f, centerY))
            release()
        }
        waitForOffer()
        rule.onNodeWithText("Cite").assertIsDisplayed()

        rule.onNodeWithText("Another message").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Cite").fetchSemanticsNodes().isEmpty() }
        rule.onAllNodesWithText("Cite").assertCountEquals(0)
    }

    private fun waitForOffer() {
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Cite").fetchSemanticsNodes().isNotEmpty() }
    }

    private fun dragAcrossTheMessage() {
        rule.waitUntil(5_000) { rule.onAllNodesWithText("The retry delay is read from RESTORE_BACKOFF_MS").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("The retry delay is read from RESTORE_BACKOFF_MS").performMouseInput {
            moveTo(Offset(1f, centerY))
            press()
            moveTo(Offset(width * 0.5f, centerY))
            moveTo(Offset(width - 1f, centerY))
            release()
        }
    }

    private fun showMessage(attached: MutableList<Annotation> = mutableListOf(), item: ThreadItem = ThreadItem.AgentMessage("e1", "The retry delay is read from RESTORE_BACKOFF_MS")) {
        rule.setContent {
            MyceliumTheme(true) {
                CompositionLocalProvider(LocalAnnotate provides AnnotateActions(onAttach = { attached += it }, onAsk = { _, _ -> }), LocalClipboardManager provides memory) {
                    ThreadView(listOf(item), onAnswer = { _, _ -> }, onStopWorkflow = {})
                }
            }
        }
    }

    @Test
    fun theOfferAppearsAsSoonAsTextIsSelectedWithoutCopyingAndLeavesTheClipboardAlone() {
        showMessage()

        dragAcrossTheMessage()

        waitForOffer()
        rule.onNodeWithText("Cite").assertIsDisplayed()
        rule.onNodeWithText("Copy").assertIsDisplayed()
        assertEquals("RESTORE_BACKOFF_MS", memory.getText()?.text)
    }

    @Test
    fun escapeDismissesTheOffer() {
        showMessage()

        dragAcrossTheMessage()
        waitForOffer()
        rule.onNodeWithText("Cite").assertIsDisplayed()
        rule.onAllNodes(isRoot())[0].performKeyInput { pressKey(Key.Escape) }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Cite").fetchSemanticsNodes().isEmpty() }
        rule.onAllNodesWithText("Cite").assertCountEquals(0)
    }

    @Test
    fun theShortcutAnnotatesTheSelectionWithoutTheMouse() {
        val attached = mutableListOf<Annotation>()
        showMessage(attached)

        dragAcrossTheMessage()
        rule.onAllNodes(isRoot())[0].performKeyInput {
            keyDown(Key.CtrlLeft)
            keyDown(Key.ShiftLeft)
            pressKey(Key.A)
            keyUp(Key.ShiftLeft)
            keyUp(Key.CtrlLeft)
        }

        assertEquals(1, attached.size)
        assertEquals("", attached.single().comment)
    }

    @Test
    fun selectingInsideACodeBlockOffersAnnotation() {
        showMessage(item = ThreadItem.AgentMessage("e1", "```\nRESTORE_BACKOFF_MS = 500\n```"))

        rule.onNodeWithText("RESTORE_BACKOFF_MS = 500").performMouseInput {
            moveTo(Offset(1f, centerY))
            press()
            moveTo(Offset(width * 0.5f, centerY))
            moveTo(Offset(width - 1f, centerY))
            release()
        }

        waitForOffer()
        rule.onNodeWithText("Cite").assertIsDisplayed()
    }

    @Test
    fun selectingInsideADiffOffersAnnotation() {
        val diff = FileDiff("Retry.kt", listOf(DiffLine(DiffLine.Kind.Added, null, 1, "val RESTORE_BACKOFF_MS = 5000")))
        showMessage(item = ThreadItem.ToolCall("k", "t1", "Edit", "edit", "Retry.kt", ToolStatus.Done, diff, null))

        rule.onNodeWithText("val RESTORE_BACKOFF_MS = 5000").performMouseInput {
            moveTo(Offset(1f, centerY))
            press()
            moveTo(Offset(width * 0.5f, centerY))
            moveTo(Offset(width - 1f, centerY))
            release()
        }

        waitForOffer()
        rule.onNodeWithText("Cite").assertIsDisplayed()
    }

    @Test
    fun selectingInsideCommandOutputOffersAnnotation() {
        showMessage(item = ThreadItem.ToolCall("k", "t1", "Run", "execute", "cargo test", ToolStatus.Done, null, "test result: ok. 12 passed"))

        rule.onNodeWithText("test result: ok. 12 passed").performMouseInput {
            moveTo(Offset(1f, centerY))
            press()
            moveTo(Offset(width * 0.5f, centerY))
            moveTo(Offset(width - 1f, centerY))
            release()
        }

        waitForOffer()
        rule.onNodeWithText("Cite").assertIsDisplayed()
    }

    @Test
    fun aCitedChipSurvivesNewEventsAndTheConversationChangingStatus() {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        var status = "done"
        var events = listOf(chunk(1, "The retry delay is read from RESTORE_BACKOFF_MS"))
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val conversation = """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/app","title":"Fix the build","status":"$status","acp_session_id":"s","mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null}"""
            val after = request.url.parameters["after"]?.toLongOrNull() ?: 0
            val body = when {
                path.endsWith("/conversations") -> "[$conversation]"
                path.endsWith("/conversations/c1") -> conversation
                path.endsWith("/events") -> "[" + events.drop(after.toInt()).joinToString(",") + "]"
                path.endsWith("/queue") || path.endsWith("/nodes") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking {
            store.refresh()
            store.open("c1")
        }
        rule.setContent {
            MyceliumTheme(true) {
                CompositionLocalProvider(LocalClipboardManager provides memory) {
                    Shell(store, QuietPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Chat("c1"))
                }
            }
        }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("The retry delay is read from RESTORE_BACKOFF_MS").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("The retry delay is read from RESTORE_BACKOFF_MS").performMouseInput {
            moveTo(Offset(1f, centerY))
            press()
            moveTo(Offset(width * 0.5f, centerY))
            moveTo(Offset(width - 1f, centerY))
            release()
        }
        waitForOffer()
        rule.onNodeWithText("Cite").performClick()
        rule.onNode(hasSetTextAction()).assert(holdsOneCitationMarker)

        status = "working"
        events = events + chunk(2, " and it is 500")
        runBlocking {
            store.refresh()
            store.open("c1")
        }
        rule.waitForIdle()
        status = "done"
        runBlocking { store.refresh() }
        rule.waitForIdle()

        rule.onNode(hasSetTextAction()).assert(holdsOneCitationMarker)
    }

    private fun chunk(seq: Int, text: String) =
        """{"conversation_id":"c1","seq":$seq,"kind":"acp_update","payload":{"sessionId":"s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"$text"}}}}"""
}
