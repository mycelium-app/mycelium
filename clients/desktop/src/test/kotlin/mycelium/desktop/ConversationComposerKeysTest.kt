package mycelium.desktop

import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.SemanticsNodeInteraction
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.test.withKeyDown
import androidx.compose.ui.text.TextRange
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.runBlocking
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.ConversationScreen
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private const val IdleConversation = """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/app","title":"Work","status":"done","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z"}"""

class ConversationComposerKeysTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private fun show() {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val body = when {
                path.endsWith("/conversations") -> "[$IdleConversation]"
                path.endsWith("/conversations/c1") -> IdleConversation
                path.endsWith("/events") || path.endsWith("/queue") || path.endsWith("/nodes") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        runBlocking {
            store.refresh()
            store.open("c1")
        }

        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); ConversationScreen(store, state, "c1", onArchived = {}, panels = false) } }
        rule.waitForIdle()
    }

    private fun field() = rule.onNode(hasSetTextAction())

    private fun SemanticsNodeInteraction.caret(): TextRange = fetchSemanticsNode().config.getOrNull(SemanticsProperties.TextSelectionRange) ?: TextRange.Zero

    private fun SemanticsNodeInteraction.typed(): String = fetchSemanticsNode().config.getOrNull(SemanticsProperties.EditableText)?.text.orEmpty()

    private fun SemanticsNodeInteraction.press(key: Key) = performKeyInput { pressKey(key) }

    private fun SemanticsNodeInteraction.pressWith(modifier: Key, key: Key) = performKeyInput { withKeyDown(modifier) { pressKey(key) } }

    @Test
    fun ctrlBackspacePressedTwiceDeletesOneWordPerPress() {
        show()
        field().performTextInput("one two three four")

        field().pressWith(Key.CtrlLeft, Key.Backspace)
        assertEquals("one two three ", field().typed())

        field().pressWith(Key.CtrlLeft, Key.Backspace)
        assertEquals("one two ", field().typed())
    }

    @Test
    fun ctrlLeftMovesOneWordPerPressAndKeepsMovingOnTheSecondPress() {
        show()
        field().performTextInput("one two three")

        field().pressWith(Key.CtrlLeft, Key.DirectionLeft)
        assertEquals(TextRange(8), field().caret())

        field().pressWith(Key.CtrlLeft, Key.DirectionLeft)
        assertEquals(TextRange(4), field().caret())
    }

    @Test
    fun ctrlShiftLeftSelectsMoreOnEachPress() {
        show()
        field().performTextInput("one two three")

        field().performKeyInput { withKeyDown(Key.CtrlLeft) { withKeyDown(Key.ShiftLeft) { pressKey(Key.DirectionLeft) } } }
        assertEquals(TextRange(13, 8), field().caret())

        field().performKeyInput { withKeyDown(Key.CtrlLeft) { withKeyDown(Key.ShiftLeft) { pressKey(Key.DirectionLeft) } } }
        assertEquals(TextRange(13, 4), field().caret())
    }

    @Test
    fun upAndDownArrowsMoveBetweenLinesAndKeepMovingOnRepeatedPresses() {
        show()
        field().performTextInput("one\ntwo\nthree")

        field().press(Key.DirectionUp)
        assertEquals(TextRange(7), field().caret())

        field().press(Key.DirectionUp)
        assertEquals(TextRange(3), field().caret())

        field().press(Key.DirectionDown)
        assertEquals(TextRange(7), field().caret())
    }

    @Test
    fun leftArrowMovesOneCharacterPerPress() {
        show()
        field().performTextInput("abc")

        field().press(Key.DirectionLeft)
        field().press(Key.DirectionLeft)

        assertEquals(TextRange(1), field().caret())
    }

    private fun showWithBadge(tail: String) {
        show()
        field().performTextInput("one two ")
        field().performTextInput((1..30).joinToString("\n") { "pasted $it" })
        field().performTextInput(tail)
    }

    @Test
    fun ctrlLeftStepsOverAPastedBadgeAsOneWordAndKeepsGoing() {
        showWithBadge(" three four")

        field().pressWith(Key.CtrlLeft, Key.DirectionLeft)
        assertEquals(TextRange(16), field().caret())

        field().pressWith(Key.CtrlLeft, Key.DirectionLeft)
        assertEquals(TextRange(10), field().caret())

        field().pressWith(Key.CtrlLeft, Key.DirectionLeft)
        assertEquals(TextRange(8), field().caret())

        field().pressWith(Key.CtrlLeft, Key.DirectionLeft)
        assertEquals(TextRange(4), field().caret())
    }

    @Test
    fun ctrlRightStepsOverAPastedBadgeAsOneWordAndKeepsGoing() {
        showWithBadge(" three four")
        field().pressWith(Key.CtrlLeft, Key.MoveHome)

        field().pressWith(Key.CtrlLeft, Key.DirectionRight)
        assertEquals(TextRange(3), field().caret())

        field().pressWith(Key.CtrlLeft, Key.DirectionRight)
        assertEquals(TextRange(7), field().caret())

        field().pressWith(Key.CtrlLeft, Key.DirectionRight)
        assertEquals(TextRange(9), field().caret())

        field().pressWith(Key.CtrlLeft, Key.DirectionRight)
        assertEquals(TextRange(15), field().caret())
    }

    @Test
    fun ctrlShiftLeftSelectsMoreAcrossAPastedBadgeOnEachPress() {
        showWithBadge(" three four")

        field().performKeyInput { withKeyDown(Key.CtrlLeft) { withKeyDown(Key.ShiftLeft) { pressKey(Key.DirectionLeft) } } }
        assertEquals(TextRange(20, 16), field().caret())

        field().performKeyInput { withKeyDown(Key.CtrlLeft) { withKeyDown(Key.ShiftLeft) { pressKey(Key.DirectionLeft) } } }
        assertEquals(TextRange(20, 10), field().caret())

        field().performKeyInput { withKeyDown(Key.CtrlLeft) { withKeyDown(Key.ShiftLeft) { pressKey(Key.DirectionLeft) } } }
        assertEquals(TextRange(20, 8), field().caret())
    }

    @Test
    fun ctrlBackspaceDeletesTheWordBeforeAPastedBadgeOnEachPress() {
        showWithBadge(" three four")

        field().pressWith(Key.CtrlLeft, Key.Backspace)
        assertEquals(" three ", field().typed().takeLast(7))
    }
}
