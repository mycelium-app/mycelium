package mycelium.desktop

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.performClick
import mycelium.state.BreakClock
import mycelium.state.BreakPhase
import mycelium.ui.BreakToast
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

class BreakToastTest {
    @get:Rule
    val rule = createComposeRule()

    private val due = BreakClock(BreakPhase.Due, activeMinutes = 50)

    @Test
    fun snoozeAndStartBreakAreButtons() {
        val taken = mutableListOf<String>()
        rule.setContent { MyceliumTheme(true) { BreakToast(due, 5, { taken += "snooze" }, { taken += "break" }, { taken += "dismiss" }) } }

        rule.onNodeWithText("Time for a break").assertIsDisplayed()
        rule.onNodeWithText("Snooze").performClick()
        rule.onNodeWithText("Start break").performClick()
        rule.onNodeWithContentDescription("Dismiss").performClick()

        assertEquals(listOf("snooze", "break", "dismiss"), taken)
    }

    @Test
    fun nothingShowsUnlessABreakIsDue() {
        rule.setContent { MyceliumTheme(true) { BreakToast(BreakClock(), 5, {}, {}, {}) } }

        assertEquals(0, rule.onAllNodesWithText("Time for a break").fetchSemanticsNodes().size)
    }
}
