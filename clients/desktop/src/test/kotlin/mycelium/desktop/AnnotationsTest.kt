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
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performMouseInput
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.onRoot
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
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.runBlocking
import mycelium.api.Conversation
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.Route
import mycelium.ui.Shell
import mycelium.api.ConversationStatus
import mycelium.state.Annotation
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

class AnnotationsTest {
    @get:Rule
    val rule = createComposeRule()

    private val conversation = Conversation("c1", "u", "n", "claude", "/work", "t", ConversationStatus.Done, createdAt = "2026-10-22T10:00:00Z", updatedAt = "2026-10-22T10:00:00Z")

    private fun composer(annotations: List<Annotation>, onSend: (String) -> Unit = {}, onRemove: (Annotation) -> Unit = {}) {
        rule.setContent {
            MyceliumTheme(true) {
                Composer(
                    conversation = conversation, working = false, queue = emptyList(), sending = emptyList(), onSend = onSend, onStop = {},
                    onSendQueuedNow = {}, onEditQueued = { _, _ -> }, onRemoveQueued = {},
                    options = SessionOptions(emptyList(), null, emptyList(), null, emptyList()), commands = emptyList(),
                    onSetMode = {}, onSetModel = {}, onSetSetting = { _, _ -> }, onSearchFiles = { emptyList() },
                    annotations = annotations, onRemoveAnnotation = onRemove,
                )
            }
        }
    }

    @Test
    fun attachedAnnotationsShowAsChipsAboveTheComposerAndCanBeRemoved() {
        val removed = mutableListOf<Annotation>()
        val attached = Annotation("the retry delay", "make this 5000")
        composer(listOf(attached), onRemove = { removed += it })

        rule.onNodeWithContentDescription("Attached: \"the retry delay\": make this 5000").assertIsDisplayed()
        rule.onNodeWithContentDescription("Remove attachment: \"the retry delay\": make this 5000").performClick()

        assertEquals(listOf(attached), removed)
    }

    @Test
    fun theSendButtonSendsAttachmentsEvenWithoutTypedText() {
        val sent = mutableListOf<String>()
        composer(listOf(Annotation("the retry delay", "")), onSend = { sent += it })

        rule.onNodeWithContentDescription("Send").performClick()

        assertEquals(listOf(""), sent)
    }

    private val memory = object : ClipboardManager {
        private var held: AnnotatedString? = AnnotatedString("RESTORE_BACKOFF_MS")

        override fun setText(annotatedString: AnnotatedString) {
            held = annotatedString
        }

        override fun getText(): AnnotatedString? = held
    }

    @Test
    fun selectingTextInAMessageOffersToAddItToTheNextMessageWithAComment() {
        val attached = mutableListOf<Annotation>()
        rule.setContent {
            MyceliumTheme(true) {
                CompositionLocalProvider(LocalAnnotate provides AnnotateActions(onAttach = { attached += it }, onAsk = { _, _ -> }), LocalClipboardManager provides memory) {
                    ThreadView(listOf(ThreadItem.AgentMessage("e1", "The retry delay is read from RESTORE_BACKOFF_MS")), onAnswer = { _, _ -> }, onStopWorkflow = {})
                }
            }
        }

        rule.onNodeWithText("The retry delay is read from RESTORE_BACKOFF_MS").performMouseInput {
            press()
            moveTo(Offset(width * 0.9f, centerY))
            release()
        }
        rule.onRoot().performKeyInput { pressKey(Key.Tab) }
        rule.onRoot().performKeyInput { pressKey(Key.Tab) }
        rule.onRoot().performKeyInput {
            keyDown(Key.CtrlLeft)
            pressKey(Key.C)
            keyUp(Key.CtrlLeft)
        }
        rule.onNodeWithText("Add to message").assertIsDisplayed().performClick()
        rule.onNode(hasSetTextAction()).performTextInput("make this 5000")
        rule.onNodeWithText("Add").performClick()

        assertEquals(1, attached.size)
        assertEquals(Annotation("RESTORE_BACKOFF_MS", "make this 5000"), attached.single())
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

        rule.onNodeWithText("The retry delay is read from RESTORE_BACKOFF_MS").performMouseInput {
            press()
            moveTo(Offset(width * 0.9f, centerY))
            release()
        }
        rule.onRoot().performKeyInput { pressKey(Key.Tab) }
        rule.onRoot().performKeyInput { pressKey(Key.Tab) }
        rule.onRoot().performKeyInput {
            keyDown(Key.CtrlLeft)
            pressKey(Key.C)
            keyUp(Key.CtrlLeft)
        }
        rule.onNodeWithText("Add to message").assertIsDisplayed()

        rule.runOnIdle { focus!!.clearFocus() }
        rule.waitForIdle()

        rule.onNodeWithText("Add to message").assertIsDisplayed()
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

        rule.onNodeWithText("The retry delay is read from RESTORE_BACKOFF_MS").performMouseInput {
            press()
            moveTo(Offset(width * 0.9f, centerY))
            release()
        }
        rule.onRoot().performKeyInput { pressKey(Key.Tab) }
        rule.onRoot().performKeyInput { pressKey(Key.Tab) }
        rule.onRoot().performKeyInput {
            keyDown(Key.CtrlLeft)
            pressKey(Key.C)
            keyUp(Key.CtrlLeft)
        }
        rule.onNodeWithText("Add to message").assertIsDisplayed()

        rule.onNodeWithText("Another message").performClick()

        rule.onAllNodesWithText("Add to message").assertCountEquals(0)
    }

    @Test
    fun anAttachedChipSurvivesNewEventsAndTheConversationChangingStatus() {
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
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), CoroutineScope(Dispatchers.Unconfined))
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

        rule.onNodeWithText("The retry delay is read from RESTORE_BACKOFF_MS").performMouseInput {
            press()
            moveTo(Offset(width * 0.9f, centerY))
            release()
        }
        repeat(20) {
            if (rule.onAllNodesWithText("Add to message").fetchSemanticsNodes().isEmpty()) {
                rule.onRoot().performKeyInput { pressKey(Key.Tab) }
                rule.onRoot().performKeyInput {
                    keyDown(Key.CtrlLeft)
                    pressKey(Key.C)
                    keyUp(Key.CtrlLeft)
                }
            }
        }
        rule.onNodeWithText("Add to message").performClick()
        rule.onNode(hasSetTextAction() and hasAnyAncestor(hasContentDescription("Add to message"))).performTextInput("make this 5000")
        rule.onNodeWithText("Add").performClick()
        rule.onNodeWithContentDescription("Attached: \"RESTORE_BACKOFF_MS\": make this 5000").assertIsDisplayed()

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

        rule.onNodeWithContentDescription("Attached: \"RESTORE_BACKOFF_MS\": make this 5000").assertIsDisplayed()
    }

    private fun chunk(seq: Int, text: String) =
        """{"conversation_id":"c1","seq":$seq,"kind":"acp_update","payload":{"sessionId":"s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"$text"}}}}"""
}
