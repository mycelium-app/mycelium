package mycelium.desktop

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.width
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.getUnclippedBoundsInRoot
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.unit.dp
import mycelium.ui.ButtonVariant
import mycelium.ui.MyButton
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class ButtonLabelsTest {
    @get:Rule
    val rule = createComposeRule()

    private fun overflows(text: String): Boolean {
        val results = mutableListOf<TextLayoutResult>()
        val node = rule.onNodeWithText(text).fetchSemanticsNode()
        node.config[SemanticsActions.GetTextLayoutResult].action?.invoke(results)
        val layout = results.first()

        return layout.isLineEllipsized(0) || layout.size.width > node.size.width
    }

    @Test
    fun a_button_row_in_a_narrow_window_wraps_and_shows_every_label_in_full() {
        rule.setContent {
            MyceliumTheme {
                FlowRow(Modifier.width(220.dp), horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.End), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    MyButton({}, text = "Move after this turn")
                    MyButton({}, text = "Cancel move")
                    MyButton({}, text = "Start the exercise", variant = ButtonVariant.Primary)
                }
            }
        }

        listOf("Move after this turn", "Cancel move", "Start the exercise").forEach { assertFalse(overflows(it), "$it is cut off") }

        val first = rule.onNodeWithText("Move after this turn").getUnclippedBoundsInRoot()
        val last = rule.onNodeWithText("Start the exercise").getUnclippedBoundsInRoot()
        assertTrue(last.top >= first.bottom, "the primary button moves to a new line")
    }
}
