package mycelium.desktop

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlinx.coroutines.runBlocking
import mycelium.Connection
import mycelium.DeviceInfo
import mycelium.Platform
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.WellnessScreen
import org.junit.Rule

internal class OpeningPlatform : Platform {
    val opened = mutableListOf<String>()

    override val signsInThroughTheBrowser = false
    override val originUrl: String? = null

    override fun loadConnection(): Connection? = null
    override fun saveConnection(connection: Connection?) = Unit
    override fun device() = DeviceInfo("test", "test", "linux", "x86_64")
    override fun approveCode(): String? = null
    override fun currentPath() = "/"
    override fun navigate(url: String) = Unit
    override fun openUrl(url: String) {
        opened += url
    }
    override fun copyToClipboard(text: String) = Unit
}

class WellnessScreenTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val requests = WellnessRequests()
    private val platform = OpeningPlatform()

    private fun show() {
        val store = Store(requests.api(), scopes.create())

        runBlocking { store.wellness.load() }
        rule.setContent { MyceliumTheme(true) { WellnessScreen(store, platform) } }
        rule.waitUntil(5_000) { rule.onAllNodesWithText("With agents today").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun todayShowsTimeWithAgentsTimeLookingAtSessionsAndBreaks() {
        show()

        rule.onNodeWithText("3 h 10 min").assertIsDisplayed()
        rule.onNodeWithText("2 h 15 min").assertIsDisplayed()
    }

    @Test
    fun theChartSaysWhatWasHeldWhileClosedThisWeek() {
        show()

        rule.onNodeWithText("11 notifications", substring = true).assertIsDisplayed()
    }

    @Test
    fun theWeekChartIsDescribedForAssistiveTechnology() {
        show()

        rule.onNodeWithContentDescription("Last 7 days", substring = true).assertExists()
    }

    @Test
    fun aPuzzleOpensItsSiteAndPlayedTodayIsSelfReported() {
        show()

        rule.onNodeWithText("Puzzles").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Open Wordle").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithContentDescription("Open Wordle").performClick()
        assertEquals(listOf("https://www.nytimes.com/games/wordle/index.html"), platform.opened)

        rule.onNodeWithContentDescription("Connections played today").performClick()
        rule.waitUntil(5_000) { requests.sent("PUT", "/wellness/puzzles/connections/today") != null }
    }
}
