package mycelium.desktop

import androidx.compose.ui.input.key.Key
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.SemanticsNodeInteraction
import androidx.compose.ui.test.assertTextEquals
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.test.withKeyDown
import androidx.compose.ui.text.TextRange
import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.state.SessionOptions
import mycelium.state.pastedBlock
import mycelium.ui.Composer
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

class ComposerKeyboardTest {
    @get:Rule
    val rule = createComposeRule()

    private val long = (1..20).joinToString("\n") { "line $it" }

    private fun show(sent: MutableList<String> = mutableListOf(), files: List<String> = emptyList()) {
        val conversation = Conversation("c1", "u", "n", "claude", "/work", "t", ConversationStatus.Done, createdAt = "2026-10-22T10:00:00Z", updatedAt = "2026-10-22T10:00:00Z")
        rule.setContent {
            MyceliumTheme(true) {
                Composer(
                    conversation = conversation, working = false, queue = emptyList(), sending = emptyList(), onSend = { sent += it }, onStop = {},
                    onSendQueuedNow = {}, onEditQueued = {}, onRemoveQueued = {},
                    options = SessionOptions(emptyList(), null, emptyList(), null, emptyList()), commands = emptyList(),
                    onSetMode = {}, onSetModel = {}, onSetSetting = { _, _ -> }, onSearchFiles = { files },
                )
            }
        }
    }

    private fun field() = rule.onNode(hasSetTextAction())

    private fun SemanticsNodeInteraction.caret(): TextRange = fetchSemanticsNode().config.getOrNull(SemanticsProperties.TextSelectionRange) ?: TextRange.Zero

    private fun SemanticsNodeInteraction.press(key: Key) = performKeyInput { pressKey(key) }

    private fun SemanticsNodeInteraction.pressWith(modifier: Key, key: Key) = performKeyInput { withKeyDown(modifier) { pressKey(key) } }

    @Test
    fun upArrowMovesTheCaretToThePreviousLineInsteadOfDoingNothingElse() {
        val sent = mutableListOf<String>()
        show(sent)

        field().performTextInput("one\ntwo")
        field().press(Key.DirectionUp)

        assertEquals(TextRange(3), field().caret())
        field().assertTextEquals("one\ntwo")
        assertEquals(emptyList(), sent)
    }

    @Test
    fun downArrowMovesTheCaretBackToTheNextLine() {
        show()

        field().performTextInput("one\ntwo")
        field().press(Key.DirectionUp)
        field().press(Key.DirectionDown)

        assertEquals(TextRange(7), field().caret())
    }

    @Test
    fun upArrowOnTheFirstLineLeavesTheTextAlone() {
        val sent = mutableListOf<String>()
        show(sent)

        field().performTextInput("only line")
        field().press(Key.DirectionUp)

        field().assertTextEquals("only line")
        assertEquals(emptyList(), sent)
    }

    @Test
    fun typingAfterMovingUpInsertsAtTheCaretNotTheEnd() {
        show()

        field().performTextInput("one\ntwo")
        field().press(Key.DirectionUp)
        field().performTextInput("X")

        field().assertTextEquals("oneX\ntwo")
    }

    @Test
    fun shiftEnterInsertsANewLineAndDoesNotSend() {
        val sent = mutableListOf<String>()
        show(sent)

        field().performTextInput("one")
        field().pressWith(Key.ShiftLeft, Key.Enter)
        field().performTextInput("two")

        field().assertTextEquals("one\ntwo")
        assertEquals(emptyList(), sent)
    }

    @Test
    fun enterSendsTheWholeMultiLineMessageWithoutAddingALine() {
        val sent = mutableListOf<String>()
        show(sent)

        field().performTextInput("one\ntwo")
        field().press(Key.DirectionUp)
        field().press(Key.Enter)

        assertEquals(listOf("one\ntwo"), sent)
        field().assertTextEquals("")
    }

    @Test
    fun enterOnWhitespaceOnlyDoesNotSend() {
        val sent = mutableListOf<String>()
        show(sent)

        field().performTextInput("  ")
        field().press(Key.Enter)

        assertEquals(emptyList(), sent)
    }

    @Test
    fun homeAndEndMoveWithinTheCurrentLine() {
        show()

        field().performTextInput("one\ntwo")
        field().press(Key.DirectionUp)
        field().press(Key.MoveHome)

        assertEquals(TextRange(0), field().caret())

        field().press(Key.MoveEnd)

        assertEquals(TextRange(3), field().caret())
    }

    @Test
    fun ctrlHomeAndCtrlEndJumpToTheWholeMessageEnds() {
        show()

        field().performTextInput("one\ntwo\nthree")
        field().pressWith(Key.CtrlLeft, Key.MoveHome)

        assertEquals(TextRange(0), field().caret())

        field().pressWith(Key.CtrlLeft, Key.MoveEnd)

        assertEquals(TextRange(13), field().caret())
    }

    @Test
    fun shiftUpExtendsTheSelectionAndTypingReplacesIt() {
        show()

        field().performTextInput("one\ntwo")
        field().performKeyInput { withKeyDown(Key.ShiftLeft) { pressKey(Key.DirectionUp) } }

        assertEquals(TextRange(7, 3), field().caret())

        field().performTextInput("-")

        field().assertTextEquals("one-")
    }

    @Test
    fun selectAllThenTypingReplacesTheMessage() {
        show()

        field().performTextInput("one\ntwo")
        field().pressWith(Key.CtrlLeft, Key.A)
        field().performTextInput("new")

        field().assertTextEquals("new")
    }

    @Test
    fun undoRevertsTypingAndRedoRestoresIt() {
        show()

        field().performTextInput("hello world")
        field().pressWith(Key.CtrlLeft, Key.Z)

        field().assertTextEquals("")

        field().performKeyInput { withKeyDown(Key.CtrlLeft) { withKeyDown(Key.ShiftLeft) { pressKey(Key.Z) } } }

        field().assertTextEquals("hello world")
    }

    @Test
    fun undoAfterMovingTheCaretUpStillUndoesTheTyping() {
        show()

        field().performTextInput("one\ntwo")
        field().press(Key.DirectionUp)
        field().performTextInput("X")
        field().pressWith(Key.CtrlLeft, Key.Z)

        field().assertTextEquals("one\ntwo")
    }

    @Test
    fun leftArrowStepsOverAnInlinePasteBadgeAsOneCharacter() {
        show()

        field().performTextInput("see ")
        field().performTextInput(long)
        field().performTextInput(" end")
        field().press(Key.DirectionLeft)
        field().press(Key.DirectionLeft)
        field().press(Key.DirectionLeft)
        field().press(Key.DirectionLeft)

        assertEquals(TextRange(5), field().caret())

        field().press(Key.DirectionLeft)

        assertEquals(TextRange(4), field().caret())
    }

    @Test
    fun upArrowWithAPasteBadgeOnTheFirstLineDoesNotDisturbTheBadgeOrTheText() {
        val sent = mutableListOf<String>()
        show(sent)

        field().performTextInput("see ")
        field().performTextInput(long)
        field().performTextInput("\nafter")
        field().press(Key.DirectionUp)
        field().press(Key.DirectionDown)
        field().press(Key.Enter)

        assertEquals(listOf("see ${pastedBlock(long)}\nafter"), sent)
    }

    @Test
    fun typingNextToABadgeKeepsTheBadgeWhole() {
        val sent = mutableListOf<String>()
        show(sent)

        field().performTextInput(long)
        field().performTextInput("tail")
        field().press(Key.MoveHome)
        field().performTextInput("head ")
        field().press(Key.Enter)

        assertEquals(listOf("head ${pastedBlock(long)}tail"), sent)
    }

    @Test
    fun upAndDownMoveThroughFileMatchesWithoutMovingTheCaretOrSending() {
        val sent = mutableListOf<String>()
        show(sent, files = listOf("a.kt", "b.kt", "c.kt"))

        field().performTextInput("look at @")
        rule.waitUntil(5_000) { rule.onAllNodes(androidx.compose.ui.test.hasContentDescription("b.kt")).fetchSemanticsNodes().isNotEmpty() }
        field().press(Key.DirectionDown)
        field().press(Key.Enter)

        assertEquals(emptyList(), sent)
        field().assertTextEquals("look at @b.kt ")
    }

    @Test
    fun escapeInTheFileMatchesClosesThemAndKeepsTheMessage() {
        show(files = listOf("a.kt", "b.kt"))

        field().performTextInput("a long message mentioning @")
        rule.waitUntil(5_000) { rule.onAllNodes(androidx.compose.ui.test.hasContentDescription("a.kt")).fetchSemanticsNodes().isNotEmpty() }
        field().press(Key.Escape)

        field().assertTextEquals("a long message mentioning @")
    }
}
