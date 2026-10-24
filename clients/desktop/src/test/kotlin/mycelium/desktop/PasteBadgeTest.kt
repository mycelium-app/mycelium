package mycelium.desktop

import androidx.compose.ui.input.key.Key
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.test.assertTextEquals
import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.onNodeWithText
import mycelium.state.SessionOptions
import mycelium.state.ThreadItem
import mycelium.ui.ThreadView
import mycelium.state.pastedBlock
import mycelium.ui.Composer
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

class PasteBadgeTest {
    @get:Rule
    val rule = createComposeRule()

    private val long = (1..20).joinToString("\n") { "line $it" }

    private fun show(sent: MutableList<String>) {
        val conversation = Conversation("c1", "u", "n", "claude", "/work", "t", ConversationStatus.Done, createdAt = "2026-10-22T10:00:00Z", updatedAt = "2026-10-22T10:00:00Z")
        rule.setContent {
            MyceliumTheme(true) {
                Composer(
                    conversation = conversation, working = false, queue = emptyList(), sending = emptyList(), onSend = { sent += it }, onStop = {},
                    onSendQueuedNow = {}, onEditQueued = { _, _ -> }, onRemoveQueued = {},
                    options = SessionOptions(emptyList(), null, emptyList(), null, emptyList()), commands = emptyList(),
                    onSetMode = {}, onSetModel = {}, onSetSetting = { _, _ -> }, onSearchFiles = { emptyList() },
                )
            }
        }
    }

    @Test
    fun aLongPasteShowsInlineAndIsSentAsAPastedTextBlock() {
        val sent = mutableListOf<String>()
        show(sent)
        val field = rule.onNode(hasSetTextAction())

        field.performTextInput("see ")
        field.performTextInput(long)

        rule.onNodeWithText("see Pasted text · 20 lines").assertExists()

        field.performKeyInput { pressKey(Key.Enter) }

        assertEquals(listOf("see ${pastedBlock(long)}"), sent)
    }

    @Test
    fun aShortPasteStaysPlainText() {
        show(mutableListOf())

        rule.onNode(hasSetTextAction()).performTextInput("one\ntwo")

        rule.onNodeWithText("Pasted text · 2 lines", substring = true).assertDoesNotExist()
    }

    @Test
    fun backspaceAfterAnInlinePasteDeletesTheWholePaste() {
        val sent = mutableListOf<String>()
        show(sent)
        val field = rule.onNode(hasSetTextAction())

        field.performTextInput("see ")
        field.performTextInput(long)
        field.performKeyInput { pressKey(Key.Backspace) }

        rule.onNodeWithText("Pasted text · 20 lines", substring = true).assertDoesNotExist()
        field.assertTextEquals("see ")
        assertEquals(emptyList(), sent)
    }

    @Test
    fun aSentPastedBlockShowsAsACollapsedBadgeThatExpandsInPlace() {
        rule.setContent {
            MyceliumTheme(true) {
                ThreadView(listOf(ThreadItem.UserMessage("u1", "look at ${pastedBlock(long)} please")), onAnswer = { _, _ -> }, onStopWorkflow = {})
            }
        }

        rule.onNodeWithText("line 7", substring = true).assertDoesNotExist()
        rule.onNodeWithText("look at", substring = true).assertIsDisplayed()

        rule.onNodeWithContentDescription("Pasted text · 20 lines").performClick()

        rule.onNodeWithText("line 7", substring = true).assertExists()
    }
}
