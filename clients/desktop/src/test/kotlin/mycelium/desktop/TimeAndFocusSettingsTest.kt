package mycelium.desktop

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
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
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Quiet hours").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun choosingAnotherReminderIntervalSavesIt() {
        show()

        rule.onNodeWithText("90 min").performClick()
        rule.waitUntil(5_000) { requests.sent("PUT", "/wellness/settings") != null }
        assertTrue(requests.sent("PUT", "/wellness/settings")!!.third.contains("\"break_after_minutes\":90"))
    }

    @Test
    fun switchingAWeekdayOffSavesItAsDisabled() {
        show()

        rule.onNodeWithContentDescription("Monday quiet hours").performClick()
        rule.waitUntil(5_000) { requests.sent("PUT", "/wellness/settings") != null }
        assertTrue(requests.sent("PUT", "/wellness/settings")!!.third.contains("\"weekday\":0,\"enabled\":false"))
    }

    @Test
    fun theHoldChoicesShowWhileQuietHoursAreOn() {
        show()

        rule.onNodeWithText("Hold scheduled runs").assertExists()
        rule.onNodeWithText("Hold assistant pings").assertExists()
    }
}
