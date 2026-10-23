package mycelium.desktop

import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
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
import mycelium.api.Conversation
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
}
