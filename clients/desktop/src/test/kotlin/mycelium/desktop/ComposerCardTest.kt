package mycelium.desktop

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.SemanticsNodeInteraction
import androidx.compose.ui.test.getBoundsInRoot
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.TextFieldValue
import mycelium.ui.AttachmentControls
import mycelium.ui.ComposerCard
import mycelium.ui.ConversationKind
import mycelium.ui.MyButton
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class ComposerCardTest {
    @get:Rule
    val rule = createComposeRule()

    private fun show() {
        rule.setContent {
            var value by remember { mutableStateOf(TextFieldValue("")) }

            MyceliumTheme(true) {
                ComposerCard(
                    value = value,
                    onValueChange = { value = it },
                    placeholder = "Ask",
                    onSubmit = {},
                    attachments = AttachmentControls(emptyList(), null, {}, {}),
                    kind = ConversationKind.NewConversation,
                    strip = { MyButton({}, text = "orion", description = "Place: orion") },
                    model = { MyButton({}, text = "Opus", description = "Model: Opus") },
                )
            }
        }
    }

    private fun field() = rule.onNode(hasSetTextAction())

    private fun SemanticsNodeInteraction.typed(): String = fetchSemanticsNode().config.getOrNull(SemanticsProperties.EditableText)?.text.orEmpty()

    @Test
    fun thePlaceStripIsAttachedBelowTheComposerBoxAndTheModelPickerStaysInTheBar() {
        show()

        val box = field().getBoundsInRoot()
        val card = rule.onNodeWithContentDescription("Model: Opus").getBoundsInRoot()
        val place = rule.onNodeWithContentDescription("Place: orion").getBoundsInRoot()

        assertTrue(card.top >= box.bottom, "model picker stays in the bar below the text field")
        assertTrue(place.top >= card.bottom, "place strip sits below the card")
    }

    @Test
    fun upArrowMovesTheCaretToThePreviousLineOfTheNewConversationField() {
        show()

        field().performTextInput("one\ntwo")
        field().performKeyInput { pressKey(Key.DirectionUp) }
        field().performTextInput("X")

        assertEquals("oneX\ntwo", field().typed())
    }
}
