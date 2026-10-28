package mycelium.desktop

import androidx.compose.ui.input.key.Key
import androidx.compose.ui.test.assertTextEquals
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTextReplacement
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.test.requestFocus
import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.api.QueuedPrompt
import mycelium.state.SessionOptions
import mycelium.ui.Composer
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

class ComposerSendTest {
    @get:Rule
    val rule = createComposeRule()

    @Test
    fun aCharacterTypedJustAfterSendingStartsANewMessageInsteadOfRestoringTheSentOne() {
        val sent = mutableListOf<String>()
        val conversation = Conversation("c1", "u", "n", "claude", "/work", "t", ConversationStatus.Done, createdAt = "2026-10-22T10:00:00Z", updatedAt = "2026-10-22T10:00:00Z")
        rule.setContent {
            MyceliumTheme(true) {
                Composer(
                    conversation = conversation, working = false, queue = emptyList(), sending = emptyList(), onSend = { sent += it }, onStop = {},
                    onSendQueuedNow = {}, onEditQueued = {}, onRemoveQueued = {},
                    options = SessionOptions(emptyList(), null, emptyList(), null, emptyList()), commands = emptyList(),
                    onSetMode = {}, onSetModel = {}, onSetSetting = { _, _ -> }, onSearchFiles = { emptyList() },
                )
            }
        }
        val field = rule.onNode(hasSetTextAction())

        field.performTextInput("hello")
        field.performKeyInput { pressKey(Key.Enter) }
        field.performTextReplacement("hellox")

        assertEquals(listOf("hello"), sent)
        field.assertTextEquals("x")
    }

    @Test
    fun enterInAnEmptyComposerSendsTheNextQueuedMessageNow() {
        val sentNow = mutableListOf<String>()
        val conversation = Conversation("c1", "u", "n", "claude", "/work", "t", ConversationStatus.Working, createdAt = "2026-10-22T10:00:00Z", updatedAt = "2026-10-22T10:00:00Z")
        val queue = listOf(QueuedPrompt("q2", "c1", 2, "second", "2026-10-22T10:00:00Z"), QueuedPrompt("q1", "c1", 1, "first", "2026-10-22T10:00:00Z"))
        rule.setContent {
            MyceliumTheme(true) {
                Composer(
                    conversation = conversation, working = true, queue = queue, sending = emptyList(), onSend = {}, onStop = {},
                    onSendQueuedNow = { sentNow += it }, onEditQueued = {}, onRemoveQueued = {},
                    options = SessionOptions(emptyList(), null, emptyList(), null, emptyList()), commands = emptyList(),
                    onSetMode = {}, onSetModel = {}, onSetSetting = { _, _ -> }, onSearchFiles = { emptyList() },
                )
            }
        }

        val field = rule.onNode(hasSetTextAction())

        field.requestFocus()
        field.performKeyInput { pressKey(Key.Enter) }

        assertEquals(listOf("q1"), sentNow)
    }
}
