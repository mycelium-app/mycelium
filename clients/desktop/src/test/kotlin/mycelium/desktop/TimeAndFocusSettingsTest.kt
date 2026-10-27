package mycelium.desktop

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextReplacement
import kotlinx.coroutines.runBlocking
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.TimeAndFocusSettings
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertTrue

class TimeAndFocusSettingsTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val requests = WellnessRequests()

    private fun show() {
        val store = Store(requests.api(), scopes.create())

        runBlocking { store.wellness.load() }
        rule.setContent { MyceliumTheme(true) { TimeAndFocusSettings(store) } }
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Hours").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun choosingAnotherReminderIntervalSavesIt() {
        show()

        rule.onNodeWithText("90 min").performClick()
        rule.waitUntil(5_000) { requests.sent("PUT", "/wellness/settings") != null }
        assertTrue(requests.sent("PUT", "/wellness/settings")!!.third.contains("\"break_after_minutes\":90"))
    }

    @Test
    fun markingADayClosedAllDaySavesItAsClosed() {
        show()

        rule.onNodeWithContentDescription("Monday closed all day").performClick()
        rule.waitUntil(5_000) { requests.sent("PUT", "/wellness/settings") != null }
        assertTrue(requests.sent("PUT", "/wellness/settings")!!.third.contains("\"weekday\":0,\"closed\":true"))
    }

    @Test
    fun choosingAlwaysOpenSavesMyceliumAsAlwaysOpen() {
        show()

        rule.onNodeWithContentDescription("Mycelium hours").assertExists()
        rule.onAllNodesWithText("Always open").onFirst().performClick()
        rule.waitUntil(5_000) { requests.sent("PUT", "/wellness/settings") != null }
        assertTrue(requests.sent("PUT", "/wellness/settings")!!.third.contains("\"always_open\":true"))
    }

    @Test
    fun aClosingTimeOfTwentyFourHundredIsAcceptedAndSaved() {
        show()

        rule.onNodeWithContentDescription("Mycelium closes on Monday").performTextReplacement("24:00")
        rule.waitUntil(5_000) { requests.sent("PUT", "/wellness/settings") != null }
        assertTrue(requests.sent("PUT", "/wellness/settings")!!.third.contains("\"closes\":\"24:00\""))
    }

    @Test
    fun theGroupsTableSummarisesEachGroupsHours() {
        show()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Weekdays 09:00 to 18:00").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithText("Every group is closed while Mycelium is closed.").assertExists()
        rule.onNodeWithText("New groups are always open.").assertExists()
    }

    @Test
    fun theHoldChoicesShowUnderWhileClosed() {
        show()

        rule.onNodeWithText("While closed").assertExists()
        rule.onNodeWithText("Hold scheduled runs").assertExists()
        rule.onNodeWithText("Hold assistant pings").assertExists()
    }

    @Test
    fun onlyBreakRemindersAreLeftAndPomodoroIsGone() {
        show()

        rule.onNodeWithText("90 min").assertExists()
        assertTrue(rule.onAllNodesWithText("Pomodoro", substring = true, ignoreCase = true).fetchSemanticsNodes().isEmpty())
    }
}
