package mycelium.desktop

import androidx.compose.ui.input.key.Key
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.test.requestFocus
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.performClick
import mycelium.state.BreakClock
import mycelium.state.BreakPhase
import mycelium.state.DefaultWellnessSettings
import mycelium.ui.BreakCard
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

@OptIn(ExperimentalTestApi::class)
class BreakCardTest {
    @get:Rule
    val rule = createComposeRule()

    private val due = BreakClock(BreakPhase.Due, workedMillis = 50 * 60_000L)

    private fun button(label: String) = SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.Button) and hasText(label)

    @Test
    fun aFirstReminderOffersStartBreakAndSnoozeAndCannotBeClosed() {
        val taken = mutableListOf<String>()
        rule.setContent { MyceliumTheme(true) { BreakCard(due, 5, { taken += "snooze" }, { taken += "break" }) } }

        rule.onNodeWithText("Time for a break").assertIsDisplayed()
        assertEquals(0, rule.onAllNodesWithContentDescription("Dismiss").fetchSemanticsNodes().size)
        rule.onNode(button("Start break")).requestFocus().performKeyInput { pressKey(Key.Escape) }
        rule.onNodeWithText("Time for a break").assertIsDisplayed()
        rule.onNode(button("Snooze")).performClick()
        rule.onNode(button("Start break")).performClick()

        rule.captureScreen("break-toast-first")
        assertEquals(listOf("snooze", "break"), taken)
    }

    @Test
    fun theReminderShowsHowLongTheBreakIsOverdueInMinutesAndSeconds() {
        rule.setContent { MyceliumTheme(true) { BreakCard(due.copy(overdueMillis = 754_000L), 5, {}, {}) } }

        rule.onNodeWithText("12:34 overdue").assertIsDisplayed()
        rule.onNodeWithText("A 5 minute break will do.").assertIsDisplayed()
    }

    @Test
    fun onceTheSnoozeRunsOutOnlyStartBreakIsOffered() {
        val settings = DefaultWellnessSettings
        val back = due.snooze(settings).copy(countedAt = 0).advance(settings, false, settings.snoozeMinutes * 60_000L, null)
        rule.setContent { MyceliumTheme(true) { BreakCard(back, 5, {}, {}) } }

        rule.onNode(button("Start break")).assertIsDisplayed()
        assertEquals(0, rule.onAllNodes(button("Snooze")).fetchSemanticsNodes().size)
        assertEquals(0, rule.onAllNodesWithContentDescription("Dismiss").fetchSemanticsNodes().size)
        rule.captureScreen("break-toast-after-snooze")
    }

    @Test
    fun nothingShowsUnlessABreakIsDue() {
        rule.setContent { MyceliumTheme(true) { BreakCard(BreakClock(), 5, {}, {}) } }

        assertEquals(0, rule.onAllNodesWithText("Time for a break").fetchSemanticsNodes().size)
    }
}
